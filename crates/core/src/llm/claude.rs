use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use super::{ChatRequest, LlmClient};
use crate::config::{LlmConfig, SecretKey};
use crate::error::{AgentError, Result};

const ANTHROPIC_VERSION: &str = "2023-06-01";
const MAX_TOKENS: u32 = 4096;

/// Anthropic Messages API client (`/v1/messages`). Claude has no native JSON
/// mode, so `json_mode` is enforced via the system prompt.
pub struct ClaudeClient {
    http: reqwest::Client,
    base_url: String,
    model: String,
    api_key: SecretKey,
}

#[derive(Deserialize)]
struct MessagesResponse {
    content: Vec<ContentBlock>,
}

#[derive(Deserialize)]
struct ContentBlock {
    #[serde(default)]
    text: String,
}

impl ClaudeClient {
    pub fn new(http: reqwest::Client, config: &LlmConfig) -> Self {
        Self {
            http,
            base_url: config.base_url.trim_end_matches('/').to_string(),
            model: config.model.clone(),
            api_key: config.api_key.clone(),
        }
    }

    fn system_prompt(&self, request: &ChatRequest) -> String {
        if request.json_mode {
            format!(
                "{}\n\nRespond with a single valid JSON value and nothing else.",
                request.system
            )
        } else {
            request.system.clone()
        }
    }
}

#[async_trait]
impl LlmClient for ClaudeClient {
    async fn complete(&self, request: &ChatRequest) -> Result<String> {
        let url = format!("{}/v1/messages", self.base_url);
        let body = json!({
            "model": self.model,
            "max_tokens": MAX_TOKENS,
            "system": self.system_prompt(request),
            "messages": [{"role": "user", "content": request.user}],
        });

        let response = self
            .http
            .post(&url)
            .header("x-api-key", self.api_key.expose())
            .header("anthropic-version", ANTHROPIC_VERSION)
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(AgentError::LlmResponse(format!(
                "claude returned HTTP {}",
                response.status()
            )));
        }
        let parsed: MessagesResponse = response.json().await?;
        let text: String = parsed.content.into_iter().map(|block| block.text).collect();
        if text.is_empty() {
            return Err(AgentError::LlmResponse(
                "claude response had no text content".into(),
            ));
        }
        Ok(text)
    }
}
