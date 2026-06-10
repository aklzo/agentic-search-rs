mod duckduckgo;
mod searxng;

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;

use crate::config::{SearchConfig, SearchProviderKind};
use crate::error::Result;

/// One search engine result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchHit {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

/// Provider-agnostic web search interface.
#[async_trait]
pub trait SearchProvider: Send + Sync {
    async fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>>;
}

pub fn build_provider(config: &SearchConfig) -> Result<Arc<dyn SearchProvider>> {
    let http = http_client()?;
    let provider: Arc<dyn SearchProvider> = match config.provider {
        SearchProviderKind::DuckDuckGo => Arc::new(duckduckgo::DuckDuckGo::new(http)),
        SearchProviderKind::SearxNg => {
            Arc::new(searxng::SearxNg::new(http, config.searxng_base_url.clone()))
        }
    };
    Ok(provider)
}

fn http_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .connect_timeout(Duration::from_secs(10))
        .user_agent(crate::fetch::USER_AGENT)
        .build()?)
}
