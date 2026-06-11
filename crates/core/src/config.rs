use std::fmt;

use crate::error::{AgentError, Result};

/// API key wrapper that never appears in logs or debug output.
#[derive(Clone, Default)]
pub struct SecretKey(String);

impl SecretKey {
    pub fn new(value: String) -> Self {
        Self(value)
    }

    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for SecretKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretKey(***)")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LlmProviderKind {
    Ollama,
    Claude,
    OpenAi,
}

impl LlmProviderKind {
    pub fn parse(value: &str) -> Result<Self> {
        match value.to_ascii_lowercase().as_str() {
            "ollama" => Ok(Self::Ollama),
            "claude" | "anthropic" => Ok(Self::Claude),
            "openai" => Ok(Self::OpenAi),
            other => Err(AgentError::Config(format!(
                "unknown LLM provider '{other}' (expected: ollama, claude, openai)"
            ))),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchProviderKind {
    DuckDuckGo,
    SearxNg,
}

impl SearchProviderKind {
    pub fn parse(value: &str) -> Result<Self> {
        match value.to_ascii_lowercase().as_str() {
            "duckduckgo" | "ddg" => Ok(Self::DuckDuckGo),
            "searxng" => Ok(Self::SearxNg),
            other => Err(AgentError::Config(format!(
                "unknown search provider '{other}' (expected: duckduckgo, searxng)"
            ))),
        }
    }
}

#[derive(Clone, Debug)]
pub struct LlmConfig {
    pub provider: LlmProviderKind,
    pub model: String,
    pub base_url: String,
    pub api_key: SecretKey,
    pub timeout_secs: u64,
}

#[derive(Clone, Debug)]
pub struct SearchConfig {
    pub provider: SearchProviderKind,
    pub searxng_base_url: String,
}

/// Hard limits that bound the agent's autonomy (cost, runtime, memory).
#[derive(Clone, Debug)]
pub struct Limits {
    pub max_iterations: u32,
    pub max_queries_per_iteration: usize,
    pub max_results_per_query: usize,
    pub max_pages_per_query: usize,
    pub max_content_chars: usize,
    pub fetch_timeout_secs: u64,
    pub max_response_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_iterations: 4,
            max_queries_per_iteration: 4,
            max_results_per_query: 8,
            max_pages_per_query: 3,
            max_content_chars: 6_000,
            fetch_timeout_secs: 20,
            max_response_bytes: 2 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Config {
    pub llm: LlmConfig,
    pub search: SearchConfig,
    pub limits: Limits,
    /// Language the final report is written in (`AGS_REPORT_LANGUAGE`).
    pub report_language: String,
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

impl Config {
    /// Build configuration from environment variables. A provider passed on
    /// the command line takes precedence over `AGS_LLM_PROVIDER`.
    pub fn from_env(provider_override: Option<LlmProviderKind>) -> Result<Self> {
        let provider = match provider_override {
            Some(provider) => provider,
            None => LlmProviderKind::parse(&env_or("AGS_LLM_PROVIDER", "ollama"))?,
        };
        let llm = LlmConfig {
            provider,
            model: env_or("AGS_LLM_MODEL", default_model(provider)),
            base_url: env_or("AGS_LLM_BASE_URL", default_base_url(provider)),
            api_key: SecretKey::new(read_api_key(provider)),
            timeout_secs: 180,
        };

        let search = SearchConfig {
            provider: SearchProviderKind::parse(&env_or("AGS_SEARCH_PROVIDER", "duckduckgo"))?,
            searxng_base_url: env_or("AGS_SEARXNG_URL", "http://localhost:8080"),
        };

        let config = Self {
            llm,
            search,
            limits: Limits::default(),
            report_language: env_or("AGS_REPORT_LANGUAGE", "日本語"),
        };
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<()> {
        let needs_key = !matches!(self.llm.provider, LlmProviderKind::Ollama);
        if needs_key && self.llm.api_key.is_empty() {
            return Err(AgentError::Config(format!(
                "provider {:?} requires an API key ({})",
                self.llm.provider,
                api_key_env_name(self.llm.provider)
            )));
        }
        Ok(())
    }
}

fn default_model(provider: LlmProviderKind) -> &'static str {
    match provider {
        LlmProviderKind::Ollama => "llama3.2:3b",
        LlmProviderKind::Claude => "claude-sonnet-4-6",
        LlmProviderKind::OpenAi => "gpt-4o-mini",
    }
}

/// Default API base URL per provider (also used by frontends, e.g. to query
/// the local Ollama server for installed models).
pub fn default_base_url(provider: LlmProviderKind) -> &'static str {
    match provider {
        LlmProviderKind::Ollama => "http://localhost:11434",
        LlmProviderKind::Claude => "https://api.anthropic.com",
        LlmProviderKind::OpenAi => "https://api.openai.com",
    }
}

fn api_key_env_name(provider: LlmProviderKind) -> &'static str {
    match provider {
        LlmProviderKind::Ollama => "",
        LlmProviderKind::Claude => "ANTHROPIC_API_KEY",
        LlmProviderKind::OpenAi => "OPENAI_API_KEY",
    }
}

fn read_api_key(provider: LlmProviderKind) -> String {
    let name = api_key_env_name(provider);
    if name.is_empty() {
        return String::new();
    }
    std::env::var(name).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_key_is_redacted_in_debug_output() {
        let key = SecretKey::new("sk-super-secret".to_string());
        let printed = format!("{key:?}");
        assert!(!printed.contains("super-secret"));
        assert!(printed.contains("***"));
    }

    #[test]
    fn provider_parsing_accepts_known_names() {
        assert_eq!(
            LlmProviderKind::parse("Ollama").unwrap(),
            LlmProviderKind::Ollama
        );
        assert_eq!(
            LlmProviderKind::parse("anthropic").unwrap(),
            LlmProviderKind::Claude
        );
        assert!(LlmProviderKind::parse("gemini").is_err());
    }

    #[test]
    fn search_provider_parsing() {
        assert_eq!(
            SearchProviderKind::parse("ddg").unwrap(),
            SearchProviderKind::DuckDuckGo
        );
        assert!(SearchProviderKind::parse("bing").is_err());
    }
}
