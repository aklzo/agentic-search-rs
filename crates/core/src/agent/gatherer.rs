use chrono::Utc;
use serde::Deserialize;
use tracing::{debug, warn};

use super::knowledge::{Finding, KnowledgeStore};
use super::prompts;
use crate::config::Limits;
use crate::error::Result;
use crate::events::AgentEvent;
use crate::fetch::PageFetcher;
use crate::llm::{ChatRequest, LlmClient};
use crate::search::{SearchHit, SearchProvider};

#[derive(Debug, Deserialize)]
struct ExtractedFinding {
    statement: String,
    #[serde(default)]
    published_hint: Option<String>,
}

/// Executes one search query end-to-end: search, fetch unvisited pages,
/// extract findings into the store. Returns how many new findings were added.
pub struct Gatherer<'a> {
    pub llm: &'a dyn LlmClient,
    pub search: &'a dyn SearchProvider,
    pub fetcher: &'a dyn PageFetcher,
    pub limits: &'a Limits,
    pub events: Option<&'a (dyn Fn(AgentEvent) + Send + Sync)>,
}

impl Gatherer<'_> {
    pub async fn gather(
        &self,
        question: &str,
        query: &str,
        store: &mut KnowledgeStore,
    ) -> Result<usize> {
        let hits = self
            .search
            .search(query, self.limits.max_results_per_query)
            .await?;
        debug!(query, hits = hits.len(), "search complete");

        let mut new_findings = 0;
        let mut pages_processed = 0;
        for hit in hits {
            if pages_processed >= self.limits.max_pages_per_query {
                break;
            }
            if store.is_visited(&hit.url) {
                continue;
            }
            store.mark_visited(&hit.url);
            match self.process_hit(question, &hit, store).await {
                Ok(added) => {
                    pages_processed += 1;
                    new_findings += added;
                    if let Some(events) = self.events {
                        events(AgentEvent::PageProcessed {
                            url: hit.url.clone(),
                            new_findings: added,
                        });
                    }
                }
                // One bad page must not abort the whole research run.
                Err(err) => warn!(url = %hit.url, error = %err, "skipping page"),
            }
        }
        Ok(new_findings)
    }

    async fn process_hit(
        &self,
        question: &str,
        hit: &SearchHit,
        store: &mut KnowledgeStore,
    ) -> Result<usize> {
        let page = self.fetcher.fetch(&hit.url).await?;
        if page.text.trim().is_empty() {
            return Ok(0);
        }
        let extracted = self
            .extract_findings(question, &page.url, &page.text)
            .await?;
        let mut added = 0;
        for item in extracted {
            let finding = Finding {
                statement: item.statement,
                source_url: page.url.clone(),
                source_title: hit.title.clone(),
                published_hint: item.published_hint,
                retrieved_at: Utc::now(),
            };
            if store.add_finding(finding) {
                added += 1;
            }
        }
        Ok(added)
    }

    async fn extract_findings(
        &self,
        question: &str,
        url: &str,
        page_text: &str,
    ) -> Result<Vec<ExtractedFinding>> {
        let request = ChatRequest {
            system: prompts::extractor_system(),
            user: prompts::extractor_user(question, url, page_text),
            json_mode: true,
        };
        let value = self.llm.complete_json(&request).await?;
        Ok(parse_extraction(value))
    }
}

/// Lenient parsing of extractor output: small local models occasionally emit
/// a bare array or a few malformed entries; salvage every valid finding
/// instead of discarding the whole page.
fn parse_extraction(value: serde_json::Value) -> Vec<ExtractedFinding> {
    let items = match value {
        serde_json::Value::Array(items) => items,
        serde_json::Value::Object(mut map) => match map.remove("findings") {
            Some(serde_json::Value::Array(items)) => items,
            _ => return Vec::new(),
        },
        _ => return Vec::new(),
    };
    items
        .into_iter()
        .filter_map(|item| serde_json::from_value(item).ok())
        .filter(|finding: &ExtractedFinding| !finding.statement.trim().is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_wrapped_findings_object() {
        let value = serde_json::json!({
            "findings": [{"statement": "Fact", "published_hint": "2026-01-01"}]
        });
        let findings = parse_extraction(value);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].statement, "Fact");
    }

    #[test]
    fn parses_bare_array_output() {
        let value = serde_json::json!([{"statement": "Fact"}]);
        assert_eq!(parse_extraction(value).len(), 1);
    }

    #[test]
    fn salvages_valid_entries_among_malformed_ones() {
        let value = serde_json::json!({
            "findings": ["not an object", {"statement": "Good"}, {"statement": "  "}]
        });
        let findings = parse_extraction(value);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].statement, "Good");
    }

    #[test]
    fn irrelevant_shapes_yield_no_findings() {
        assert!(parse_extraction(serde_json::json!({"other": 1})).is_empty());
        assert!(parse_extraction(serde_json::json!("text")).is_empty());
    }
}
