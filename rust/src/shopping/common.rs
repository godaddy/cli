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
    let base_url = crate::environments::resolve(&ctx.middleware.env)?.api_url;
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

pub(crate) fn currency_code(value: &str) -> std::result::Result<String, String> {
    let normalized = value.trim().to_ascii_uppercase();
    if normalized.len() == 3
        && normalized
            .chars()
            .all(|character| character.is_ascii_alphabetic())
    {
        Ok(normalized)
    } else {
        Err("currency must be a three-letter ISO 4217 code".to_owned())
    }
}

pub(crate) fn merge_context_currency(request: &mut Value, currency: Option<&str>) -> Result<()> {
    let Some(currency) = currency else {
        return Ok(());
    };
    let object = request
        .as_object_mut()
        .expect("read_json validates the request is an object");
    let context = object
        .entry("context")
        .or_insert_with(|| serde_json::json!({}));
    let context = context
        .as_object_mut()
        .ok_or_else(|| GddyError::validation("context must be a JSON object").into_cli_error())?;
    if let Some(existing) = context.get("currency").and_then(Value::as_str)
        && !existing.eq_ignore_ascii_case(currency)
    {
        return Err(GddyError::validation(
            "--currency conflicts with context.currency in the request body",
        )
        .into_cli_error());
    }
    context.insert("currency".to_owned(), Value::String(currency.to_owned()));
    Ok(())
}

pub(crate) fn require_selected_payment_instrument(body: &Value) -> Result<()> {
    let selected = body
        .pointer("/payment/instruments")
        .and_then(Value::as_array)
        .map(|instruments| {
            instruments
                .iter()
                .filter(|instrument| {
                    instrument.get("selected").and_then(Value::as_bool) == Some(true)
                })
                .collect::<Vec<_>>()
        });
    if let Some([instrument]) = selected.as_deref()
        && instrument
            .get("id")
            .and_then(Value::as_str)
            .is_some_and(|id| !id.trim().is_empty())
    {
        Ok(())
    } else {
        Err(GddyError::validation(
            "checkout completion requires exactly one selected saved payment instrument with an ID",
        )
        .with_fix(
            "Include payment.instruments with exactly one saved instrument ID marked selected: true.",
        )
        .into_cli_error())
    }
}

/// Returns a supplied non-empty key, or inserts a new UUID for this one request.
pub(crate) fn ensure_completion_idempotency_key(body: &mut Value) -> Result<String> {
    let object = body
        .as_object_mut()
        .expect("read_json validates the completion request is an object");
    match object.get("idempotency_key") {
        None => {
            let key = uuid::Uuid::new_v4().to_string();
            object.insert("idempotency_key".to_owned(), Value::String(key.clone()));
            Ok(key)
        }
        Some(Value::String(key)) if !key.trim().is_empty() => Ok(key.clone()),
        Some(_) => Err(GddyError::validation(
            "idempotency_key must be a non-empty string when supplied",
        )
        .with_fix("Supply a non-empty idempotency_key, or omit it to let gddy generate one.")
        .into_cli_error()),
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

pub(crate) fn wait_duration(seconds: Option<u8>) -> Result<Duration> {
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
    use serde_json::json;

    use super::*;

    #[test]
    fn generates_and_inserts_missing_completion_idempotency_key() {
        let mut request = json!({});
        let key = ensure_completion_idempotency_key(&mut request).expect("key should be generated");

        assert!(uuid::Uuid::parse_str(&key).is_ok());
        assert_eq!(request["idempotency_key"], key);
    }

    #[test]
    fn preserves_supplied_completion_idempotency_key() {
        let mut request = json!({"idempotency_key": "customer-key"});

        assert_eq!(
            ensure_completion_idempotency_key(&mut request).expect("key should be valid"),
            "customer-key"
        );
    }

    #[test]
    fn rejects_blank_completion_idempotency_key() {
        let mut request = json!({"idempotency_key": "  "});

        assert!(ensure_completion_idempotency_key(&mut request).is_err());
    }

    #[test]
    fn requires_exactly_one_selected_payment_instrument() {
        assert!(
            require_selected_payment_instrument(&json!({
                "payment": {"instruments": [{"id": "payment-1", "selected": true}]}
            }))
            .is_ok()
        );
        assert!(
            require_selected_payment_instrument(&json!({
                "payment": {"instruments": []}
            }))
            .is_err()
        );
        assert!(
            require_selected_payment_instrument(&json!({
                "payment": {"instruments": [{"selected": true}, {"selected": true}]}
            }))
            .is_err()
        );
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
