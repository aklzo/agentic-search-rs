use std::collections::HashSet;

use chrono::{DateTime, Utc};
use serde::Serialize;
use sha2::{Digest, Sha256};

/// One verified-as-relevant statement extracted from a source page.
#[derive(Clone, Debug, Serialize)]
pub struct Finding {
    pub statement: String,
    pub source_url: String,
    pub source_title: String,
    /// Publication date as stated by the source, if the extractor saw one.
    pub published_hint: Option<String>,
    pub retrieved_at: DateTime<Utc>,
}

/// Accumulated research state: deduplicated findings plus the URLs and
/// queries already consumed, so the loop never repeats work.
#[derive(Default)]
pub struct KnowledgeStore {
    findings: Vec<Finding>,
    statement_hashes: HashSet<[u8; 32]>,
    visited_urls: HashSet<String>,
    executed_queries: HashSet<String>,
}

impl KnowledgeStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a finding unless an equivalent statement is already stored.
    /// Returns `true` when the finding was new (the novelty signal).
    pub fn add_finding(&mut self, finding: Finding) -> bool {
        let hash = statement_hash(&finding.statement);
        if !self.statement_hashes.insert(hash) {
            return false;
        }
        self.findings.push(finding);
        true
    }

    pub fn findings(&self) -> &[Finding] {
        &self.findings
    }

    pub fn mark_visited(&mut self, url: &str) -> bool {
        self.visited_urls.insert(url.to_string())
    }

    pub fn is_visited(&self, url: &str) -> bool {
        self.visited_urls.contains(url)
    }

    /// Record a query as executed. Returns `false` if it ran before.
    pub fn mark_query(&mut self, query: &str) -> bool {
        self.executed_queries.insert(normalize(query))
    }

    pub fn source_count(&self) -> usize {
        self.findings
            .iter()
            .map(|finding| finding.source_url.as_str())
            .collect::<HashSet<_>>()
            .len()
    }

    /// Compact numbered digest of all findings for evaluator/reporter prompts.
    pub fn digest(&self, max_chars: usize) -> String {
        let mut out = String::new();
        for (index, finding) in self.findings.iter().enumerate() {
            let date = finding.published_hint.as_deref().unwrap_or("date unknown");
            let line = format!(
                "[{}] {} (source: {} | {})\n",
                index + 1,
                finding.statement,
                finding.source_url,
                date
            );
            if out.chars().count() + line.chars().count() > max_chars {
                out.push_str("... (digest truncated)\n");
                break;
            }
            out.push_str(&line);
        }
        out
    }
}

fn statement_hash(statement: &str) -> [u8; 32] {
    Sha256::digest(normalize(statement)).into()
}

/// Case/whitespace-insensitive form used for deduplication.
fn normalize(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finding(statement: &str, url: &str) -> Finding {
        Finding {
            statement: statement.to_string(),
            source_url: url.to_string(),
            source_title: "title".to_string(),
            published_hint: None,
            retrieved_at: Utc::now(),
        }
    }

    #[test]
    fn deduplicates_equivalent_statements() {
        let mut store = KnowledgeStore::new();
        assert!(store.add_finding(finding("Rust 1.95 was released", "https://a.example")));
        assert!(!store.add_finding(finding("rust  1.95 WAS released", "https://b.example")));
        assert_eq!(store.findings().len(), 1);
    }

    #[test]
    fn tracks_visited_urls_and_queries() {
        let mut store = KnowledgeStore::new();
        assert!(store.mark_visited("https://a.example"));
        assert!(store.is_visited("https://a.example"));
        assert!(store.mark_query("rust async runtime"));
        assert!(!store.mark_query("Rust   ASYNC runtime"));
    }

    #[test]
    fn digest_lists_findings_with_sources() {
        let mut store = KnowledgeStore::new();
        store.add_finding(finding("Fact one", "https://a.example"));
        store.add_finding(finding("Fact two", "https://b.example"));
        let digest = store.digest(500);
        assert!(digest.contains("[1] Fact one"));
        assert!(digest.contains("https://b.example"));
        assert_eq!(store.source_count(), 2);
    }

    #[test]
    fn digest_respects_char_budget() {
        let mut store = KnowledgeStore::new();
        for index in 0..50 {
            store.add_finding(finding(
                &format!("A reasonably long statement number {index}"),
                "https://a.example",
            ));
        }
        let digest = store.digest(300);
        assert!(digest.chars().count() < 400);
        assert!(digest.contains("truncated"));
    }
}
