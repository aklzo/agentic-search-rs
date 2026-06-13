use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use super::{SearchHit, SearchProvider};
use crate::config::SecretKey;
use crate::error::{AgentError, Result};

const ENDPOINT: &str = "https://google.serper.dev/search";

/// Serper.dev client: a keyed wrapper over Google search results, returning
/// title/link/snippet in the same shape as [`SearchHit`]. High rate limits
/// (≈300 QPS) make it the production-grade alternative to scraping
/// DuckDuckGo when parallel/high-frequency search is needed.
pub struct Serper {
    http: reqwest::Client,
    api_key: SecretKey,
}

#[derive(Deserialize)]
struct SerperResponse {
    #[serde(default)]
    organic: Vec<OrganicResult>,
}

#[derive(Deserialize)]
struct OrganicResult {
    #[serde(default)]
    title: String,
    link: String,
    #[serde(default)]
    snippet: String,
}

impl Serper {
    pub fn new(http: reqwest::Client, api_key: SecretKey) -> Self {
        Self { http, api_key }
    }
}

#[async_trait]
impl SearchProvider for Serper {
    async fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>> {
        let response = self
            .http
            .post(ENDPOINT)
            .header("X-API-KEY", self.api_key.expose())
            .json(&json!({ "q": query, "num": limit }))
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(AgentError::Search(format!(
                "serper returned HTTP {}",
                response.status()
            )));
        }
        let parsed: SerperResponse = response.json().await?;
        Ok(parsed
            .organic
            .into_iter()
            .take(limit)
            .map(|result| SearchHit {
                title: result.title,
                url: result.link,
                snippet: result.snippet,
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_organic_results() {
        let body = json!({
            "organic": [
                {"title": "T1", "link": "https://a.example", "snippet": "s1"},
                {"title": "T2", "link": "https://b.example"}
            ]
        });
        let parsed: SerperResponse = serde_json::from_value(body).unwrap();
        assert_eq!(parsed.organic.len(), 2);
        assert_eq!(parsed.organic[0].link, "https://a.example");
        assert_eq!(
            parsed.organic[1].snippet, "",
            "missing snippet defaults to empty"
        );
    }
}
