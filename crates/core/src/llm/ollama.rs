use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use super::{ChatRequest, LlmClient};
use crate::config::LlmConfig;
use crate::error::{AgentError, Result};

/// Context window requested from Ollama. The server default (4096 tokens)
/// silently truncates this tool's prompts: page extraction feeds ~6,000 chars
/// and the evaluator digest up to 12,000 chars, which exceeds 4K tokens for
/// Japanese text. Sized to fit the largest prompt with headroom.
const NUM_CTX: u32 = 16_384;

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
            "options": {"temperature": 0.2, "num_ctx": NUM_CTX},
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{LlmProviderKind, SecretKey};

    #[test]
    fn request_body_sets_context_window_and_json_format() {
        let config = crate::config::LlmConfig {
            provider: LlmProviderKind::Ollama,
            model: "test-model".into(),
            base_url: "http://localhost:11434".into(),
            api_key: SecretKey::default(),
            timeout_secs: 1,
        };
        let client = OllamaClient::new(reqwest::Client::new(), &config);
        let body = client.build_body(&crate::llm::ChatRequest {
            system: "s".into(),
            user: "u".into(),
            json_mode: true,
        });
        // Without an explicit num_ctx Ollama silently truncates long prompts
        // to its 4K default, which breaks the evaluator digest.
        assert_eq!(body["options"]["num_ctx"], NUM_CTX);
        assert_eq!(body["format"], "json");
    }
}
