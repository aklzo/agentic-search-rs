mod claude;
mod json;
mod ollama;
mod openai;

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;

pub use json::extract_json;
pub use ollama::list_models as list_ollama_models;

use crate::config::{LlmConfig, LlmProviderKind};
use crate::error::Result;
use crate::retry;

/// A single chat completion request. `json_mode` asks the provider to return
/// strictly parseable JSON (enforced natively where supported, by prompt otherwise).
#[derive(Clone, Debug)]
pub struct ChatRequest {
    pub system: String,
    pub user: String,
    pub json_mode: bool,
}

/// Provider-agnostic LLM interface. Implementations exist for Ollama (local,
/// default), Claude, and OpenAI; new providers only need this one method.
#[async_trait]
pub trait LlmClient: Send + Sync {
    async fn complete(&self, request: &ChatRequest) -> Result<String>;

    /// Convenience wrapper: complete in JSON mode and parse the result.
    async fn complete_json(&self, request: &ChatRequest) -> Result<serde_json::Value> {
        let text = self.complete(request).await?;
        extract_json(&text)
    }
}

pub fn build_client(config: &LlmConfig, max_retries: u32) -> Result<Arc<dyn LlmClient>> {
    let http = http_client(config.timeout_secs)?;
    let client: Arc<dyn LlmClient> = match config.provider {
        LlmProviderKind::Ollama => Arc::new(ollama::OllamaClient::new(http, config)),
        LlmProviderKind::Claude => Arc::new(claude::ClaudeClient::new(http, config)),
        LlmProviderKind::OpenAi => Arc::new(openai::OpenAiClient::new(http, config)),
    };
    if max_retries == 0 {
        return Ok(client);
    }
    Ok(Arc::new(RetryingLlm {
        inner: client,
        max_retries,
    }))
}

/// Decorator adding transient-error retry to any [`LlmClient`]. Concurrent
/// extraction calls raise the odds of a transient timeout/5xx, which a short
/// backoff absorbs.
struct RetryingLlm {
    inner: Arc<dyn LlmClient>,
    max_retries: u32,
}

#[async_trait]
impl LlmClient for RetryingLlm {
    async fn complete(&self, request: &ChatRequest) -> Result<String> {
        retry::with_backoff(self.max_retries, retry::BASE_DELAY, || {
            self.inner.complete(request)
        })
        .await
    }
}

fn http_client(timeout_secs: u64) -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .connect_timeout(Duration::from_secs(10))
        .build()?)
}

/// Extract the provider's error body for diagnostics, truncated char-safely.
/// Error bodies contain only the provider's error JSON (never credentials).
pub(crate) async fn error_body(response: reqwest::Response) -> String {
    let text = response.text().await.unwrap_or_default();
    let trimmed = text.trim();
    match trimmed.char_indices().nth(500) {
        Some((index, _)) => format!("{}…", &trimmed[..index]),
        None => trimmed.to_string(),
    }
}
