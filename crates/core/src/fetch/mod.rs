mod extract;
pub mod guard;

use std::time::Duration;

use async_trait::async_trait;
use url::Url;

pub use extract::html_to_text;

use crate::config::Limits;
use crate::error::{AgentError, Result};

pub const USER_AGENT: &str = concat!("agentic-search/", env!("CARGO_PKG_VERSION"));

const MAX_REDIRECTS: usize = 5;

/// A fetched page reduced to plain text.
#[derive(Clone, Debug)]
pub struct PageContent {
    pub url: String,
    pub text: String,
}

/// Abstraction over page retrieval so the agent loop can be tested offline.
#[async_trait]
pub trait PageFetcher: Send + Sync {
    async fn fetch(&self, url: &str) -> Result<PageContent>;
}

/// Real HTTP fetcher with SSRF guard, redirect re-validation, response size
/// cap, and timeout.
pub struct HttpFetcher {
    http: reqwest::Client,
    max_content_chars: usize,
    max_response_bytes: usize,
}

impl HttpFetcher {
    pub fn new(limits: &Limits) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(limits.fetch_timeout_secs))
            .connect_timeout(Duration::from_secs(10))
            .user_agent(USER_AGENT)
            .redirect(redirect_policy())
            .build()?;
        Ok(Self {
            http,
            max_content_chars: limits.max_content_chars,
            max_response_bytes: limits.max_response_bytes,
        })
    }

    async fn read_capped(&self, mut response: reqwest::Response) -> Result<Vec<u8>> {
        let mut body: Vec<u8> = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            if body.len() + chunk.len() > self.max_response_bytes {
                body.extend_from_slice(&chunk[..self.max_response_bytes - body.len()]);
                break;
            }
            body.extend_from_slice(&chunk);
        }
        Ok(body)
    }
}

/// Re-validate every redirect hop so a public URL cannot bounce the client
/// into a private address via `Location` headers.
fn redirect_policy() -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(|attempt| {
        if attempt.previous().len() > MAX_REDIRECTS {
            return attempt.error("too many redirects");
        }
        match guard::validate_url(attempt.url()) {
            Ok(()) => attempt.follow(),
            Err(err) => attempt.error(err.to_string()),
        }
    })
}

#[async_trait]
impl PageFetcher for HttpFetcher {
    async fn fetch(&self, raw_url: &str) -> Result<PageContent> {
        let url = Url::parse(raw_url)?;
        guard::validate_url(&url)?;
        guard::ensure_public_host(&url).await?;

        let response = self.http.get(url.clone()).send().await?;
        if !response.status().is_success() {
            return Err(AgentError::Search(format!(
                "{url} returned HTTP {}",
                response.status()
            )));
        }
        if let Some(content_type) = response.headers().get(reqwest::header::CONTENT_TYPE) {
            let value = content_type.to_str().unwrap_or_default();
            if !is_textual(value) {
                return Err(AgentError::Search(format!(
                    "{url} has unsupported content type '{value}'"
                )));
            }
        }

        let body = self.read_capped(response).await?;
        let html = String::from_utf8_lossy(&body);
        Ok(PageContent {
            url: url.to_string(),
            text: html_to_text(&html, self.max_content_chars),
        })
    }
}

fn is_textual(content_type: &str) -> bool {
    let lowered = content_type.to_ascii_lowercase();
    lowered.starts_with("text/")
        || lowered.contains("html")
        || lowered.contains("xml")
        || lowered.contains("json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn textual_content_types_are_accepted() {
        assert!(is_textual("text/html; charset=utf-8"));
        assert!(is_textual("application/xhtml+xml"));
        assert!(!is_textual("application/pdf"));
        assert!(!is_textual("image/png"));
    }
}
