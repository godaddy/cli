use std::time::{Duration, Instant};

use cli_engine::{CliCoreError, CommandContext, Result};
use serde_json::{Map, Value, json};

use crate::error::GddyError;
use crate::next_action::next_action;
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

#[derive(Debug, Clone, Default)]
pub(crate) struct CheckoutInput {
    pub(crate) items: Vec<String>,
    pub(crate) clear_items: bool,
    pub(crate) currency: Option<String>,
    pub(crate) buyer_first_name: Option<String>,
    pub(crate) buyer_last_name: Option<String>,
    pub(crate) buyer_email: Option<String>,
    pub(crate) buyer_phone: Option<String>,
    pub(crate) payment_instrument: Option<String>,
}

impl CheckoutInput {
    pub(crate) fn is_present(&self) -> bool {
        !self.items.is_empty()
            || self.clear_items
            || self.currency.is_some()
            || self.buyer_first_name.is_some()
            || self.buyer_last_name.is_some()
            || self.buyer_email.is_some()
            || self.buyer_phone.is_some()
            || self.payment_instrument.is_some()
    }

    pub(crate) fn create_body(&self) -> Result<Value> {
        if self.items.is_empty() {
            return Err(GddyError::validation(
                "checkout create requires at least one --item or a JSON request through --body or --file",
            )
            .into_cli_error());
        }
        self.body(false)
    }

    pub(crate) fn update_body(&self) -> Result<Value> {
        if self.clear_items && !self.items.is_empty() {
            return Err(
                GddyError::validation("--clear-items cannot be combined with --item")
                    .into_cli_error(),
            );
        }
        if self.items.is_empty() && !self.clear_items {
            return Err(GddyError::validation(
                "checkout update requires at least one --item or --clear-items when not using --body or --file",
            )
            .into_cli_error());
        }
        self.body(true)
    }

    pub(crate) fn completion_body(&self, idempotency_key: Option<&str>) -> Result<Value> {
        if !self.items.is_empty()
            || self.clear_items
            || self.currency.is_some()
            || self.buyer_first_name.is_some()
            || self.buyer_last_name.is_some()
            || self.buyer_email.is_some()
            || self.buyer_phone.is_some()
        {
            return Err(GddyError::validation(
                "checkout complete only supports --payment-instrument and --idempotency-key in structured mode",
            )
            .into_cli_error());
        }
        let payment_instrument = nonblank_value(
            self.payment_instrument.as_deref(),
            "--payment-instrument must be non-empty",
        )?;
        let mut body = json!({
            "payment": {"instruments": [{"id": payment_instrument, "selected": true}]}
        });
        if let Some(idempotency_key) = idempotency_key {
            body.as_object_mut()
                .expect("completion body is an object")
                .insert("idempotency_key".to_owned(), json!(idempotency_key));
        }
        Ok(body)
    }

    fn body(&self, allow_empty_items: bool) -> Result<Value> {
        let mut body = Map::new();
        if !self.items.is_empty() {
            body.insert("line_items".to_owned(), line_items(&self.items)?);
        } else if allow_empty_items && self.clear_items {
            body.insert("line_items".to_owned(), json!([]));
        }
        if let Some(currency) = &self.currency {
            body.insert("context".to_owned(), json!({"currency": currency}));
        }
        let mut buyer = Map::new();
        insert_optional_nonblank(&mut buyer, "first_name", self.buyer_first_name.as_deref())?;
        insert_optional_nonblank(&mut buyer, "last_name", self.buyer_last_name.as_deref())?;
        insert_optional_nonblank(&mut buyer, "email", self.buyer_email.as_deref())?;
        insert_optional_nonblank(&mut buyer, "phone_number", self.buyer_phone.as_deref())?;
        if !buyer.is_empty() {
            body.insert("buyer".to_owned(), Value::Object(buyer));
        }
        if let Some(payment_instrument) = &self.payment_instrument {
            body.insert(
                "payment".to_owned(),
                json!({"instruments": [{"id": nonblank_value(Some(payment_instrument), "--payment-instrument must be non-empty")?, "selected": true}]}),
            );
        }
        Ok(Value::Object(body))
    }
}

fn line_items(items: &[String]) -> Result<Value> {
    items
        .iter()
        .map(|item| {
            let (id, quantity) = parse_item(item)?;
            Ok(json!({"item": {"id": id}, "quantity": quantity}))
        })
        .collect::<Result<Vec<_>>>()
        .map(Value::Array)
}

fn parse_item(value: &str) -> Result<(&str, u64)> {
    let value = value.trim();
    let (id, quantity) = match value.rsplit_once('=') {
        Some((id, quantity)) => {
            let quantity = quantity.parse::<u64>().map_err(|_| {
                GddyError::validation(format!(
                    "invalid --item {value:?}: quantity after '=' must be a positive integer"
                ))
                .into_cli_error()
            })?;
            (id, quantity)
        }
        None => (value, 1),
    };
    if id.trim().is_empty() || quantity == 0 {
        return Err(GddyError::validation(format!(
            "invalid --item {value:?}: item ID must be non-empty and quantity must be positive"
        ))
        .into_cli_error());
    }
    Ok((id.trim(), quantity))
}

fn nonblank_value<'a>(value: Option<&'a str>, error: &str) -> Result<&'a str> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| GddyError::validation(error).into_cli_error())
}

fn insert_optional_nonblank(
    object: &mut Map<String, Value>,
    key: &str,
    value: Option<&str>,
) -> Result<()> {
    if let Some(value) = value {
        object.insert(
            key.to_owned(),
            json!(nonblank_value(
                Some(value),
                &format!("--buyer-{key} must be non-empty")
            )?),
        );
    }
    Ok(())
}

pub(crate) fn currency_code(value: &str) -> std::result::Result<String, String> {
    let normalized = value.trim().to_ascii_uppercase();
    iso_currency::Currency::from_code(&normalized)
        .is_some()
        .then_some(normalized)
        .ok_or_else(|| "currency must be a valid ISO 4217 code".to_owned())
}

pub(crate) fn merge_context_currency(request: &mut Value, currency: Option<&str>) -> Result<()> {
    let Some(currency) = currency else {
        return Ok(());
    };
    let object = request
        .as_object_mut()
        .expect("read_json validates the request is an object");
    let context = object.entry("context").or_insert_with(|| json!({}));
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

pub(crate) fn reject_multiple_payment_instruments(body: &Value) -> Result<()> {
    let Some(instruments) = body.pointer("/payment/instruments") else {
        return Ok(());
    };
    let instruments = instruments.as_array().ok_or_else(|| {
        GddyError::validation("payment.instruments must be a JSON array").into_cli_error()
    })?;
    if instruments.len() > 1 {
        return Err(GddyError::validation(
            "only one payment instrument may be specified for a Shopping checkout",
        )
        .with_fix(
            "Specify one saved payment instrument, or omit payment until checkout completion.",
        )
        .into_cli_error());
    }
    Ok(())
}

pub(crate) fn require_selected_payment_instrument(body: &Value) -> Result<()> {
    reject_multiple_payment_instruments(body)?;
    let instruments = body
        .pointer("/payment/instruments")
        .and_then(Value::as_array);
    if let Some([instrument]) = instruments.map(Vec::as_slice)
        && instrument.get("selected").and_then(Value::as_bool) == Some(true)
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
        .with_fix("Include payment.instruments with one saved instrument ID marked selected: true.")
        .into_cli_error())
    }
}

pub(crate) fn reject_mixed_checkout_input(
    body: Option<&str>,
    file: Option<&str>,
    structured_input: bool,
) -> Result<()> {
    if structured_input && (body.is_some() || file.is_some()) {
        Err(GddyError::validation(
            "use either checkout flags or a JSON request through --body or --file, not both",
        )
        .into_cli_error())
    } else {
        Ok(())
    }
}

pub(crate) fn no_saved_payment_method_action(
    checkout: &Value,
    env: &str,
    account_url: &str,
) -> Option<cli_engine::NextAction> {
    checkout
        .pointer("/payment/instruments")
        .and_then(Value::as_array)
        .is_some_and(Vec::is_empty)
        .then(|| {
            let command = if matches!(env, "prod" | "production") {
                "payment-methods add".to_owned()
            } else {
                format!("--env {env} payment-methods add")
            };
            next_action(
                command,
                format!(
                    "No saved payment method is available. Add one at {account_url}/payment-methods/add-payment, then retrieve this checkout again."
                ),
            )
        })
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
    env: &str,
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
) -> CliCoreError {
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
) -> CliCoreError {
    GddyError::not_found(format!(
        "order {order_id:?} was not visible after {attempts} attempts over {} seconds",
        timeout.as_secs_f32()
    ))
    .with_fix(format!(
        "Run: gddy {}",
        crate::shopping::command_for_env(env, format!("order get {order_id} --wait"))
    ))
    .into_cli_error()
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
    use crate::shopping::command_for_env;

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
    fn builds_structured_checkout_body() {
        let body = CheckoutInput {
            items: vec!["product-a=2".to_owned(), "product-b".to_owned()],
            currency: Some("GBP".to_owned()),
            buyer_first_name: Some("Jane".to_owned()),
            buyer_email: Some("jane@example.test".to_owned()),
            payment_instrument: Some("payment-1".to_owned()),
            ..CheckoutInput::default()
        }
        .create_body()
        .expect("structured input should build");

        assert_eq!(
            body,
            json!({
                "line_items": [
                    {"item": {"id": "product-a"}, "quantity": 2},
                    {"item": {"id": "product-b"}, "quantity": 1}
                ],
                "context": {"currency": "GBP"},
                "buyer": {"first_name": "Jane", "email": "jane@example.test"},
                "payment": {"instruments": [{"id": "payment-1", "selected": true}]}
            })
        );
    }

    #[test]
    fn rejects_invalid_structured_checkout_items() {
        assert!(
            CheckoutInput {
                items: vec!["product=0".to_owned()],
                ..CheckoutInput::default()
            }
            .create_body()
            .is_err()
        );
        assert!(
            CheckoutInput {
                items: vec!["product=two".to_owned()],
                ..CheckoutInput::default()
            }
            .create_body()
            .is_err()
        );
    }

    #[test]
    fn update_requires_cart_intent_and_supports_clear_items() {
        assert!(
            CheckoutInput {
                buyer_email: Some("jane@example.test".to_owned()),
                ..CheckoutInput::default()
            }
            .update_body()
            .is_err()
        );
        assert_eq!(
            CheckoutInput {
                clear_items: true,
                ..CheckoutInput::default()
            }
            .update_body()
            .expect("clear-items should be valid"),
            json!({"line_items": []})
        );
        assert!(
            CheckoutInput {
                clear_items: true,
                items: vec!["product".to_owned()],
                ..CheckoutInput::default()
            }
            .update_body()
            .is_err()
        );
    }

    #[test]
    fn builds_structured_completion_with_one_payment_instrument() {
        let body = CheckoutInput {
            payment_instrument: Some("payment-1".to_owned()),
            ..CheckoutInput::default()
        }
        .completion_body(Some("customer-key"))
        .expect("completion body should build");

        assert_eq!(
            body,
            json!({
                "payment": {"instruments": [{"id": "payment-1", "selected": true}]},
                "idempotency_key": "customer-key"
            })
        );
    }

    #[test]
    fn rejects_mixed_checkout_input_sources() {
        assert!(reject_mixed_checkout_input(Some("{}"), None, true).is_err());
        assert!(reject_mixed_checkout_input(None, Some("request.json"), true).is_err());
        assert!(reject_mixed_checkout_input(Some("{}"), Some("request.json"), false).is_ok());
    }

    #[test]
    fn adds_payment_method_actions_for_resolved_environment_urls() {
        let empty_instruments = json!({"payment": {"instruments": []}});
        for (env, account_url, command) in [
            (
                "prod",
                "https://account.godaddy.com",
                "gddy payment-methods add",
            ),
            (
                "test",
                "https://account.test-godaddy.com",
                "gddy --env test payment-methods add",
            ),
            (
                "dev",
                "https://account.dev-godaddy.com",
                "gddy --env dev payment-methods add",
            ),
        ] {
            let action = no_saved_payment_method_action(&empty_instruments, env, account_url)
                .expect("empty list should require a payment method");
            assert_eq!(action.command, command);
            assert!(
                action
                    .description
                    .contains(&format!("{account_url}/payment-methods/add-payment"))
            );
        }
        assert!(
            no_saved_payment_method_action(
                &json!({"payment": {}}),
                "prod",
                "https://account.godaddy.com",
            )
            .is_none()
        );
    }

    #[test]
    fn requires_exactly_one_payment_instrument() {
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
                "payment": {"instruments": [
                    {"id": "payment-1", "selected": true},
                    {"id": "payment-2", "selected": false}
                ]}
            }))
            .is_err()
        );
    }

    #[test]
    fn preserves_named_environment_in_order_wait_recovery_command() {
        assert_eq!(
            command_for_env("test", "order get order-1 --wait"),
            "--env test shopping order get order-1 --wait"
        );
    }

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
