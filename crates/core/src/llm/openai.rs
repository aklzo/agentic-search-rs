use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use super::{ChatRequest, LlmClient};
use crate::config::{LlmConfig, SecretKey};
use crate::error::{AgentError, Result};

/// OpenAI Chat Completions client (`/v1/chat/completions`). Also works with
/// any OpenAI-compatible endpoint via `AGS_LLM_BASE_URL`.
pub struct OpenAiClient {
    http: reqwest::Client,
    base_url: String,
    model: String,
    api_key: SecretKey,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: ChoiceMessage,
}

#[derive(Deserialize)]
struct ChoiceMessage {
    content: Option<String>,
}

impl OpenAiClient {
    pub fn new(http: reqwest::Client, config: &LlmConfig) -> Self {
        Self {
            http,
            base_url: config.base_url.trim_end_matches('/').to_string(),
            model: config.model.clone(),
            api_key: config.api_key.clone(),
        }
    }

    fn build_body(&self, request: &ChatRequest) -> serde_json::Value {
        let mut body = json!({
            "model": self.model,
            "temperature": 0.2,
            "messages": [
                {"role": "system", "content": request.system},
                {"role": "user", "content": request.user},
            ],
        });
        if request.json_mode {
            body["response_format"] = json!({"type": "json_object"});
        }
        body
    }
}

#[async_trait]
impl LlmClient for OpenAiClient {
    async fn complete(&self, request: &ChatRequest) -> Result<String> {
        let url = format!("{}/v1/chat/completions", self.base_url);
        let response = self
            .http
            .post(&url)
            .bearer_auth(self.api_key.expose())
            .json(&self.build_body(request))
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let detail = super::error_body(response).await;
            return Err(AgentError::LlmResponse(format!(
                "openai returned HTTP {status}: {detail}"
            )));
        }
        let parsed: ChatResponse = response.json().await?;
        parsed
            .choices
            .into_iter()
            .next()
            .and_then(|choice| choice.message.content)
            .ok_or_else(|| AgentError::LlmResponse("openai response had no content".into()))
    }
}
