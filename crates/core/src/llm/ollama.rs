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

#[derive(Deserialize)]
struct TagsResponse {
    #[serde(default)]
    models: Vec<TaggedModel>,
}

#[derive(Deserialize)]
struct TaggedModel {
    name: String,
}

/// List models installed on the local Ollama server (`/api/tags`), sorted by
/// name. Used by frontends to offer a model picker.
pub async fn list_models(base_url: &str) -> Result<Vec<String>> {
    let url = format!("{}/api/tags", base_url.trim_end_matches('/'));
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()?;
    let response = http.get(&url).send().await?;
    if !response.status().is_success() {
        return Err(AgentError::LlmResponse(format!(
            "ollama returned HTTP {} for /api/tags",
            response.status()
        )));
    }
    let parsed: TagsResponse = response.json().await?;
    let mut names: Vec<String> = parsed.models.into_iter().map(|model| model.name).collect();
    names.sort();
    Ok(names)
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
            let status = response.status();
            let model = self.model.clone();
            let detail = super::error_body(response).await;
            return Err(AgentError::LlmResponse(format!(
                "ollama returned HTTP {status}: {detail} (is the model pulled? `ollama pull {model}`)"
            )));
        }
        let parsed: OllamaResponse = response.json().await?;
        Ok(parsed.message.content)
    }
}
