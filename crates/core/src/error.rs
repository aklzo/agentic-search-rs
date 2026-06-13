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

impl AgentError {
    /// Whether retrying the same operation might succeed. True only for
    /// transient transport/server conditions (timeout, connection reset,
    /// 5xx, 429); deterministic failures (4xx, blocked URL, parse error,
    /// bad config) are never retried.
    pub fn is_retryable(&self) -> bool {
        match self {
            AgentError::Http(err) => {
                err.is_timeout()
                    || err.is_connect()
                    || matches!(err.status(), Some(status) if status.is_server_error() || status.as_u16() == 429)
            }
            _ => false,
        }
    }
}

pub type Result<T> = std::result::Result<T, AgentError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_http_errors_are_not_retryable() {
        assert!(!AgentError::BlockedUrl("x".into()).is_retryable());
        assert!(!AgentError::LlmResponse("x".into()).is_retryable());
        assert!(!AgentError::Search("404".into()).is_retryable());
        assert!(!AgentError::Config("x".into()).is_retryable());
    }
}
