//! Retry helper for transient network errors during API calls.

use std::future::Future;
use std::time::Duration;

use console::style;

/// Retry an async operation up to `max_attempts` times with exponential backoff.
/// Only retries on errors that look transient (timeouts, 5xx, connection errors).
/// Prints a retry notice to stderr on each retry.
#[allow(clippy::print_stderr)]
pub(crate) async fn with_retry<F, Fut, T, E>(
    label: &str,
    max_attempts: u32,
    mut operation: F,
) -> Result<T, E>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, E>>,
    E: std::fmt::Display,
{
    let mut attempt = 0;
    loop {
        attempt += 1;
        match operation().await {
            Ok(v) => return Ok(v),
            Err(e) if attempt < max_attempts && is_retryable(&e) => {
                let delay = Duration::from_millis(1000 * 2u64.pow(attempt - 1));
                eprintln!(
                    "  {} {} failed (attempt {}/{}), retrying in {}s...",
                    style("⟳").yellow(),
                    label,
                    attempt,
                    max_attempts,
                    delay.as_secs()
                );
                tokio::time::sleep(delay).await;
            }
            Err(e) => return Err(e),
        }
    }
}

/// Heuristic: is this error likely transient?
fn is_retryable<E: std::fmt::Display>(err: &E) -> bool {
    let msg = err.to_string().to_lowercase();
    msg.contains("timeout")
        || msg.contains("timed out")
        || msg.contains("connection")
        || msg.contains("503")
        || msg.contains("502")
        || msg.contains("504")
        || msg.contains("service unavailable")
        || msg.contains("temporarily")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[tokio::test]
    async fn succeeds_on_first_attempt() {
        let result: Result<&str, String> =
            with_retry("test", 3, || async { Ok("ok") }).await;
        assert_eq!(result.expect("should succeed"), "ok");
    }

    #[tokio::test]
    async fn retries_on_transient_error() {
        let attempts = AtomicU32::new(0);
        let result: Result<&str, String> = with_retry("test", 3, || {
            let n = attempts.fetch_add(1, Ordering::SeqCst);
            async move {
                if n < 2 {
                    Err("connection timeout".to_owned())
                } else {
                    Ok("recovered")
                }
            }
        })
        .await;
        assert_eq!(result.expect("should recover"), "recovered");
        assert_eq!(attempts.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn does_not_retry_non_transient() {
        let attempts = AtomicU32::new(0);
        let result: Result<&str, String> = with_retry("test", 3, || {
            attempts.fetch_add(1, Ordering::SeqCst);
            async { Err("404 not found".to_owned()) }
        })
        .await;
        assert!(result.is_err());
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn exhausts_retries() {
        let attempts = AtomicU32::new(0);
        let result: Result<&str, String> = with_retry("test", 3, || {
            attempts.fetch_add(1, Ordering::SeqCst);
            async { Err("503 service unavailable".to_owned()) }
        })
        .await;
        assert!(result.is_err());
        assert_eq!(attempts.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn is_retryable_detects_transient_errors() {
        assert!(is_retryable(&"connection timeout"));
        assert!(is_retryable(&"503 Service Unavailable"));
        assert!(is_retryable(&"502 Bad Gateway"));
        assert!(is_retryable(&"request timed out"));
        assert!(!is_retryable(&"404 not found"));
        assert!(!is_retryable(&"401 unauthorized"));
        assert!(!is_retryable(&"invalid domain"));
    }
}
