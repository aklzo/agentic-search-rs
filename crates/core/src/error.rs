use thiserror::Error;

/// Unified error type for the whole crate.
#[derive(Debug, Error)]
pub enum AgentError {
    #[error("configuration error: {0}")]
    Config(String),

    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("URL rejected by security policy: {0}")]
    BlockedUrl(String),

    #[error("invalid URL: {0}")]
    InvalidUrl(#[from] url::ParseError),

    #[error("LLM returned an unusable response: {0}")]
    LlmResponse(String),

    #[error("failed to parse JSON: {0}")]
    Json(#[from] serde_json::Error),

    #[error("search provider error: {0}")]
    Search(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, AgentError>;
