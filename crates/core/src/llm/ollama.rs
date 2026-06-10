use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use super::{ChatRequest, LlmClient};
use crate::config::LlmConfig;
use crate::error::{AgentError, Result};

/// Local Ollama server client (`/api/chat`). The default provider: free to
/// call repeatedly, which matters during iterative agent runs.
pub struct OllamaClient {
    http: reqwest::Client,
    base_url: String,
    model: String,
}

#[derive(Deserialize)]
struct OllamaResponse {
    message: OllamaMessage,
}

#[derive(Deserialize)]
struct OllamaMessage {
    content: String,
}

impl OllamaClient {
    pub fn new(http: reqwest::Client, config: &LlmConfig) -> Self {
        Self {
            http,
            base_url: config.base_url.trim_end_matches('/').to_string(),
            model: config.model.clone(),
        }
    }

    fn build_body(&self, request: &ChatRequest) -> serde_json::Value {
        let mut body = json!({
            "model": self.model,
            "stream": false,
            "messages": [
                {"role": "system", "content": request.system},
                {"role": "user", "content": request.user},
            ],
            "options": {"temperature": 0.2},
        });
        if request.json_mode {
            body["format"] = json!("json");
        }
        body
    }
}

#[async_trait]
impl LlmClient for OllamaClient {
    async fn complete(&self, request: &ChatRequest) -> Result<String> {
        let url = format!("{}/api/chat", self.base_url);
        let response = self
            .http
            .post(&url)
            .json(&self.build_body(request))
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(AgentError::LlmResponse(format!(
                "ollama returned HTTP {} (is the model pulled? `ollama pull {}`)",
                response.status(),
                self.model
            )));
        }
        let parsed: OllamaResponse = response.json().await?;
        Ok(parsed.message.content)
    }
}
