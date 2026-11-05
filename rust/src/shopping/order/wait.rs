use std::time::Duration;

use shopping_client::types::Order;

use crate::error::GddyError;
use crate::shopping::client::{ClientError, get_order};
use crate::shopping::command_for_env;
use crate::shopping::common::client_err;

pub(crate) async fn wait_for_order(
    client: &shopping_client::Client,
    order_id: &str,
    timeout: Duration,
    env: &str,
) -> cli_engine::Result<(Order, usize)> {
    let started = std::time::Instant::now();
    let mut attempts = 0;
    let mut delay = Duration::from_secs(1);
    loop {
        attempts += 1;
        match get_order(client, order_id).await {
            Ok(order) => return Ok((order, attempts)),
            Err(error) if error.is_retryable_order_read() && started.elapsed() < timeout => {
                let remaining = timeout.saturating_sub(started.elapsed());
                let retry_delay = error.retry_after().unwrap_or(delay).min(remaining);
                if retry_delay.is_zero() {
                    return Err(exhausted_order_read_error(
                        error, order_id, attempts, timeout, env,
                    ));
                }
                tracing::debug!(
                    order_id,
                    attempts,
                    ?retry_delay,
                    "order is not visible yet; retrying"
                );
                tokio::time::sleep(retry_delay).await;
                delay = delay.saturating_mul(2).min(Duration::from_secs(4));
            }
            Err(error) => {
                return Err(exhausted_order_read_error(
                    error, order_id, attempts, timeout, env,
                ));
            }
        }
    }
}

fn exhausted_order_read_error(
    error: ClientError,
    order_id: &str,
    attempts: usize,
    timeout: Duration,
    env: &str,
) -> cli_engine::CliCoreError {
    if matches!(error, ClientError::Http { status: 404, .. }) {
        order_not_visible_error(order_id, attempts, timeout, env)
    } else {
        client_err(error)
    }
}

fn order_not_visible_error(
    order_id: &str,
    attempts: usize,
    timeout: Duration,
    env: &str,
) -> cli_engine::CliCoreError {
    GddyError::not_found(format!(
        "order {order_id:?} was not visible after {attempts} attempts over {} seconds",
        timeout.as_secs_f32()
    ))
    .with_fix(format!(
        "Run: gddy {}",
        command_for_env(env, format!("order get {order_id} --wait"))
    ))
    .into_cli_error()
}

pub(crate) fn wait_duration(seconds: Option<u8>) -> cli_engine::Result<Duration> {
    const DEFAULT: Duration = Duration::from_secs(15);
    match seconds {
        None => Ok(DEFAULT),
        Some(seconds @ 1..=60) => Ok(Duration::from_secs(u64::from(seconds))),
        Some(_) => Err(
            GddyError::validation("--wait-timeout must be between 1 and 60 seconds")
                .into_cli_error(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exhausted_order_read_maps_only_404_to_not_found() {
        let not_found = exhausted_order_read_error(
            ClientError::Http {
                status: 404,
                body: "not found".to_owned(),
                retry_after: None,
            },
            "order-1",
            1,
            Duration::ZERO,
            "test",
        );
        let rate_limited = exhausted_order_read_error(
            ClientError::Http {
                status: 429,
                body: "rate limited".to_owned(),
                retry_after: None,
            },
            "order-1",
            1,
            Duration::ZERO,
            "test",
        );

        assert!(not_found.to_string().contains("was not visible"));
        assert!(!rate_limited.to_string().contains("was not visible"));
        assert!(rate_limited.to_string().contains("429"));
    }

    #[test]
    fn validates_wait_timeout_range() {
        assert_eq!(
            wait_duration(None).expect("default"),
            Duration::from_secs(15)
        );
        assert_eq!(
            wait_duration(Some(1)).expect("lower bound"),
            Duration::from_secs(1)
        );
        assert!(wait_duration(Some(0)).is_err());
    }
}
