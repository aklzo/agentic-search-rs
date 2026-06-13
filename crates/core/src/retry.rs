//! Exponential-backoff retry for idempotent async operations.
//!
//! Used for page fetches and LLM calls, which become more failure-prone once
//! a query's pages are fetched concurrently (transient timeouts/5xx in a
//! burst). Only [`AgentError::is_retryable`](crate::error::AgentError::is_retryable)
//! errors are retried.

use std::future::Future;
use std::time::Duration;

use crate::error::Result;

/// Base delay for the first retry; doubles each attempt.
pub const BASE_DELAY: Duration = Duration::from_millis(400);

/// Run `op`, retrying up to `max_retries` extra times (so `max_retries + 1`
/// attempts total) while it returns a retryable error, sleeping
/// `base_delay * 2^attempt` between tries.
pub async fn with_backoff<T, F, Fut>(max_retries: u32, base_delay: Duration, mut op: F) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T>>,
{
    let mut attempt = 0;
    loop {
        match op().await {
            Ok(value) => return Ok(value),
            Err(err) if attempt < max_retries && err.is_retryable() => {
                let delay = base_delay.saturating_mul(2u32.saturating_pow(attempt));
                tracing::debug!(attempt = attempt + 1, error = %err, "transient error; retrying after backoff");
                tokio::time::sleep(delay).await;
                attempt += 1;
            }
            Err(err) => return Err(err),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::AgentError;
    use std::cell::Cell;

    #[tokio::test]
    async fn succeeds_without_retry() {
        let calls = Cell::new(0);
        let result = with_backoff(3, Duration::ZERO, || {
            calls.set(calls.get() + 1);
            async { Ok::<_, AgentError>(42) }
        })
        .await;
        assert_eq!(result.unwrap(), 42);
        assert_eq!(calls.get(), 1);
    }

    #[tokio::test]
    async fn does_not_retry_non_retryable_errors() {
        let calls = Cell::new(0);
        let result: Result<()> = with_backoff(3, Duration::ZERO, || {
            calls.set(calls.get() + 1);
            async { Err(AgentError::LlmResponse("bad".into())) }
        })
        .await;
        assert!(result.is_err());
        assert_eq!(calls.get(), 1, "non-retryable error must not be retried");
    }
}
