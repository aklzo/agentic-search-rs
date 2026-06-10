use async_trait::async_trait;
use serde::Deserialize;

use super::{SearchHit, SearchProvider};
use crate::error::{AgentError, Result};

/// Client for a self-hosted SearXNG instance (JSON API).
pub struct SearxNg {
    http: reqwest::Client,
    base_url: String,
}

#[derive(Deserialize)]
struct SearxResponse {
    #[serde(default)]
    results: Vec<SearxResult>,
}

#[derive(Deserialize)]
struct SearxResult {
    #[serde(default)]
    title: String,
    url: String,
    #[serde(default)]
    content: String,
}

impl SearxNg {
    pub fn new(http: reqwest::Client, base_url: String) -> Self {
        Self {
            http,
            base_url: base_url.trim_end_matches('/').to_string(),
        }
    }
}

#[async_trait]
impl SearchProvider for SearxNg {
    async fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>> {
        let url = format!("{}/search", self.base_url);
        let response = self
            .http
            .get(&url)
            .query(&[("q", query), ("format", "json")])
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(AgentError::Search(format!(
                "searxng returned HTTP {}",
                response.status()
            )));
        }
        let parsed: SearxResponse = response.json().await?;
        Ok(parsed
            .results
            .into_iter()
            .take(limit)
            .map(|result| SearchHit {
                title: result.title,
                url: result.url,
                snippet: result.content,
            })
            .collect())
    }
}
