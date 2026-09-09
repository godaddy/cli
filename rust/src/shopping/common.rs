use std::time::{Duration, Instant};

use cli_engine::{CliCoreError, CommandContext, Result};
use serde_json::Value;

use crate::error::GddyError;
use crate::shopping::SHOPPING_SCOPES;
use crate::shopping::client::{ClientError, ShoppingClient};

pub(crate) async fn make_client(ctx: &CommandContext) -> Result<ShoppingClient> {
    let required: Vec<String> = SHOPPING_SCOPES
        .iter()
        .map(|scope| (*scope).to_owned())
        .collect();
    let token = ctx.credential_with_scopes(&required).await?.token;
    let base_url = crate::environments::shopping_url(&ctx.middleware.env).ok_or_else(|| {
        GddyError::config(format!(
            "Shopping API URL is not configured for environment {:?}. Set shopping_url in \
             ~/.config/gddy/environments.toml, or set {}_SHOPPING_URL or SHOPPING_URL.",
            ctx.middleware.env,
            crate::environments::env_prefix(&ctx.middleware.env)
        ))
        .into_cli_error()
    })?;
    Ok(ShoppingClient::new(base_url, token))
}

pub(crate) fn client_err(error: ClientError) -> CliCoreError {
    GddyError::from(error).into_cli_error()
}

pub(crate) fn read_json(
    body: Option<&str>,
    file: Option<&str>,
    expected: &'static str,
) -> Result<Value> {
    let raw = if let Some(path) = file {
        std::fs::read_to_string(path).map_err(|error| {
            GddyError::validation(format!("failed to read JSON file {path:?}: {error}"))
                .into_cli_error()
        })?
    } else {
        body.unwrap_or_default().to_owned()
    };
    let value: Value = serde_json::from_str(&raw).map_err(|error| {
        GddyError::validation(format!("invalid JSON request body: {error}")).into_cli_error()
    })?;
    let valid = match expected {
        "object" => value.is_object(),
        "array" => value.is_array(),
        _ => false,
    };
    if valid {
        Ok(value)
    } else {
        Err(
            GddyError::validation(format!("request body must be a JSON {expected}"))
                .into_cli_error(),
        )
    }
}

pub(crate) fn has_conflicting_checkout_id(body: &Value, id: &str) -> bool {
    ["id", "checkout_id"]
        .iter()
        .filter_map(|key| body.get(*key).and_then(Value::as_str))
        .any(|body_id| body_id != id)
}

pub(crate) fn require_idempotency_key(body: &Value) -> Result<()> {
    if body
        .get("idempotency_key")
        .and_then(Value::as_str)
        .is_some_and(|key| !key.trim().is_empty())
    {
        Ok(())
    } else {
        Err(GddyError::validation(
            "checkout completion requires a non-empty idempotency_key in the JSON body",
        )
        .with_fix("Reuse this idempotency_key if a completion request times out.")
        .into_cli_error())
    }
}

pub(crate) async fn wait_for_order(
    client: &ShoppingClient,
    order_id: &str,
    timeout: Duration,
) -> Result<(Value, usize)> {
    let started = Instant::now();
    let mut attempts = 0;
    let mut delay = Duration::from_secs(1);
    loop {
        attempts += 1;
        match client.get_order(order_id).await {
            Ok(order) => return Ok((order, attempts)),
            Err(error) if error.is_retryable_order_read() && started.elapsed() < timeout => {
                let remaining = timeout.saturating_sub(started.elapsed());
                let retry_delay = error.retry_after().unwrap_or(delay).min(remaining);
                if retry_delay.is_zero() {
                    break;
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
            Err(error) if error.is_retryable_order_read() => {
                return Err(GddyError::not_found(format!(
                    "order {order_id:?} was not visible after {attempts} attempts over {} seconds",
                    timeout.as_secs_f32()
                ))
                .with_fix(format!("Run: gddy shopping order get {order_id} --wait"))
                .into_cli_error());
            }
            Err(error) => return Err(client_err(error)),
        }
    }
    Err(GddyError::not_found(format!(
        "order {order_id:?} was not visible after {attempts} attempts over {} seconds",
        timeout.as_secs_f32()
    ))
    .with_fix(format!("Run: gddy shopping order get {order_id} --wait"))
    .into_cli_error())
}

pub(crate) fn wait_duration(raw: Option<&str>) -> Result<Duration> {
    const DEFAULT: Duration = Duration::from_secs(15);
    let Some(raw) = raw else {
        return Ok(DEFAULT);
    };
    let seconds = raw.parse::<u64>().map_err(|_| {
        GddyError::validation("--timeout must be a whole number of seconds").into_cli_error()
    })?;
    if seconds == 0 || seconds > 60 {
        return Err(
            GddyError::validation("--timeout must be between 1 and 60 seconds").into_cli_error(),
        );
    }
    Ok(Duration::from_secs(seconds))
}
