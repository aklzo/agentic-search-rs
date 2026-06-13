use chrono::Utc;
use futures::stream::{self, StreamExt};
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

        // Select the unvisited pages to process this query (sequential, so
        // visited-marking and the per-query cap stay deterministic), then
        // fetch + extract them concurrently. Sharing only happens at the
        // merge step below, which is sequential — no locking needed.
        let mut selected = Vec::new();
        for hit in hits {
            if selected.len() >= self.limits.max_pages_per_query {
                break;
            }
            if store.is_visited(&hit.url) {
                continue;
            }
            store.mark_visited(&hit.url);
            selected.push(hit);
        }

        let concurrency = self.limits.max_concurrent_pages.max(1);
        let results = stream::iter(selected.into_iter().map(|hit| async move {
            let outcome = self.extract_page(question, &hit).await;
            (hit, outcome)
        }))
        .buffered(concurrency)
        .collect::<Vec<_>>()
        .await;

        // Merge sequentially: dedup is order-sensitive (keeps first), so a
        // single-threaded merge keeps results reproducible.
        let mut new_findings = 0;
        for (hit, outcome) in results {
            match outcome {
                Ok(findings) => {
                    let mut added = 0;
                    for finding in findings {
                        if store.add_finding(finding) {
                            added += 1;
                        }
                    }
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

    /// Fetch and extract a single page into findings. Store-independent so it
    /// can run concurrently; the caller merges results into the store.
    async fn extract_page(&self, question: &str, hit: &SearchHit) -> Result<Vec<Finding>> {
        let page = self.fetcher.fetch(&hit.url).await?;
        if page.text.trim().is_empty() {
            return Ok(Vec::new());
        }
        let extracted = self
            .extract_findings(question, &page.url, &page.text)
            .await?;
        Ok(extracted
            .into_iter()
            .map(|item| Finding {
                statement: item.statement,
                source_url: page.url.clone(),
                source_title: hit.title.clone(),
                published_hint: item.published_hint,
                retrieved_at: Utc::now(),
            })
            .collect())
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
    use crate::config::Limits;
    use crate::fetch::PageContent;
    use crate::search::SearchProvider;
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    /// Search stub returning a fixed number of distinct hits.
    struct StubSearch {
        hits: usize,
    }

    #[async_trait]
    impl SearchProvider for StubSearch {
        async fn search(&self, _query: &str, _limit: usize) -> Result<Vec<SearchHit>> {
            Ok((0..self.hits)
                .map(|i| SearchHit {
                    title: format!("t{i}"),
                    url: format!("https://example.com/{i}"),
                    snippet: String::new(),
                })
                .collect())
        }
    }

    /// Fetcher that sleeps while recording the peak number of concurrent
    /// in-flight fetches.
    struct TrackingFetcher {
        inflight: AtomicUsize,
        peak: AtomicUsize,
    }

    #[async_trait]
    impl PageFetcher for TrackingFetcher {
        async fn fetch(&self, url: &str) -> Result<PageContent> {
            let now = self.inflight.fetch_add(1, Ordering::SeqCst) + 1;
            self.peak.fetch_max(now, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(30)).await;
            self.inflight.fetch_sub(1, Ordering::SeqCst);
            Ok(PageContent {
                url: url.to_string(),
                text: "page text".into(),
            })
        }
    }

    struct StubLlm;

    #[async_trait]
    impl LlmClient for StubLlm {
        async fn complete(&self, _request: &ChatRequest) -> Result<String> {
            Ok(r#"{"findings": [{"statement": "fact"}]}"#.into())
        }
    }

    async fn peak_concurrency(max_concurrent_pages: usize) -> usize {
        let fetcher = Arc::new(TrackingFetcher {
            inflight: AtomicUsize::new(0),
            peak: AtomicUsize::new(0),
        });
        let search = StubSearch { hits: 4 };
        let llm = StubLlm;
        let limits = Limits {
            max_pages_per_query: 4,
            max_concurrent_pages,
            max_retries: 0,
            ..Limits::default()
        };
        let gatherer = Gatherer {
            llm: &llm,
            search: &search,
            fetcher: fetcher.as_ref(),
            limits: &limits,
            events: None,
        };
        let mut store = KnowledgeStore::new();
        gatherer.gather("q", "query", &mut store).await.unwrap();
        fetcher.peak.load(Ordering::SeqCst)
    }

    #[tokio::test]
    async fn pages_are_fetched_concurrently_when_allowed() {
        assert!(
            peak_concurrency(4).await > 1,
            "with a concurrency budget pages should overlap"
        );
    }

    #[tokio::test]
    async fn concurrency_one_fetches_sequentially() {
        assert_eq!(
            peak_concurrency(1).await,
            1,
            "concurrency=1 (local LLM default) must not overlap fetches"
        );
    }

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
