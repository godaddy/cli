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
    ShoppingClient::new(base_url, token).map_err(client_err)
}

pub(crate) fn client_err(error: ClientError) -> CliCoreError {
    GddyError::from(error).into_cli_error()
}

pub(crate) fn update_response(checkout: Value) -> Result<Value> {
    let errors = checkout
        .get("messages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|message| message.get("type").and_then(Value::as_str) == Some("error"))
        .filter_map(|message| message.get("content").and_then(Value::as_str))
        .collect::<Vec<_>>();
    if errors.is_empty() {
        Ok(checkout)
    } else {
        Err(GddyError::validation(errors.join(" "))
            .with_fix(
                "Review the checkout session and update its items, buyer details, currency, or selected payment method before trying again.",
            )
            .into_cli_error())
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct CheckoutInput {
    pub(crate) items: Vec<String>,
    pub(crate) currency: Option<String>,
    pub(crate) buyer_first_name: Option<String>,
    pub(crate) buyer_last_name: Option<String>,
    pub(crate) buyer_email: Option<String>,
    pub(crate) buyer_phone: Option<String>,
    pub(crate) payment_instrument: Option<String>,
}

impl CheckoutInput {
    pub(crate) fn create_body(&self) -> Result<Value> {
        if self.items.is_empty() {
            return Err(
                GddyError::validation("checkout create requires at least one --item")
                    .into_cli_error(),
            );
        }
        self.body()
    }

    pub(crate) fn update_body(&self, checkout: &Value) -> Result<Value> {
        if !self.has_update() {
            return Err(GddyError::validation(
                "checkout update requires at least one item, buyer, currency, or payment-method change",
            )
            .into_cli_error());
        }

        let mut body = writable_checkout_body(checkout)?;
        if !self.items.is_empty() {
            body.insert("line_items".to_owned(), line_items(&self.items)?);
        }
        if let Some(currency) = &self.currency {
            body["context"]["currency"] = Value::String(currency.to_owned());
        }
        if self.has_buyer_update() {
            let buyer = body
                .entry("buyer".to_owned())
                .or_insert_with(|| Value::Object(Map::new()))
                .as_object_mut()
                .expect("writable checkout buyer is an object");
            insert_optional_nonblank(buyer, "first_name", self.buyer_first_name.as_deref())?;
            insert_optional_nonblank(buyer, "last_name", self.buyer_last_name.as_deref())?;
            insert_optional_nonblank(buyer, "email", self.buyer_email.as_deref())?;
            insert_optional_nonblank(buyer, "phone_number", self.buyer_phone.as_deref())?;
        }
        if let Some(payment_instrument) = &self.payment_instrument {
            body.insert(
                "payment".to_owned(),
                payment_selection_body(nonblank_value(
                    Some(payment_instrument),
                    "--payment-instrument must be non-empty",
                )?),
            );
        }
        Ok(Value::Object(body))
    }

    fn has_update(&self) -> bool {
        !self.items.is_empty()
            || self.currency.is_some()
            || self.has_buyer_update()
            || self.payment_instrument.is_some()
    }

    fn has_buyer_update(&self) -> bool {
        self.buyer_first_name.is_some()
            || self.buyer_last_name.is_some()
            || self.buyer_email.is_some()
            || self.buyer_phone.is_some()
    }

    pub(crate) fn completion_body(&self) -> Result<Value> {
        if !self.items.is_empty()
            || self.currency.is_some()
            || self.buyer_first_name.is_some()
            || self.buyer_last_name.is_some()
            || self.buyer_email.is_some()
            || self.buyer_phone.is_some()
        {
            return Err(GddyError::validation(
                "checkout complete only supports --payment-instrument in structured mode",
            )
            .into_cli_error());
        }
        let payment_instrument = nonblank_value(
            self.payment_instrument.as_deref(),
            "--payment-instrument must be non-empty",
        )?;
        Ok(json!({
            "payment": {"instruments": [{"id": payment_instrument, "selected": true}]}
        }))
    }

    fn body(&self) -> Result<Value> {
        let mut body = Map::new();
        if !self.items.is_empty() {
            body.insert("line_items".to_owned(), line_items(&self.items)?);
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

fn writable_checkout_body(checkout: &Value) -> Result<Map<String, Value>> {
    let line_items = checkout
        .get("line_items")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            GddyError::unexpected("checkout response did not include line items").into_cli_error()
        })?
        .iter()
        .map(writable_line_item)
        .collect::<Result<Vec<_>>>()?;
    let mut body = Map::new();
    body.insert("line_items".to_owned(), Value::Array(line_items));
    let mut context = checkout
        .get("context")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    if !context.contains_key("currency")
        && let Some(currency) = checkout.get("currency").and_then(Value::as_str)
    {
        context.insert("currency".to_owned(), Value::String(currency.to_owned()));
    }
    body.insert("context".to_owned(), Value::Object(context));
    if let Some(buyer) = checkout.get("buyer").and_then(Value::as_object) {
        body.insert("buyer".to_owned(), Value::Object(buyer.clone()));
    }
    for key in ["attribution", "fulfillment", "signals"] {
        if let Some(value) = checkout.get(key) {
            body.insert(key.to_owned(), value.clone());
        }
    }
    if let Some(payment) = selected_payment(checkout) {
        body.insert("payment".to_owned(), preserved_payment_body(payment)?);
    }
    Ok(body)
}

fn writable_line_item(line_item: &Value) -> Result<Value> {
    let item_id = line_item
        .pointer("/item/id")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            GddyError::unexpected("checkout response contained a line item without an item ID")
                .into_cli_error()
        })?;
    let quantity = line_item
        .get("quantity")
        .and_then(Value::as_u64)
        .filter(|quantity| *quantity > 0)
        .ok_or_else(|| {
            GddyError::unexpected("checkout response contained a line item without a quantity")
                .into_cli_error()
        })?;
    let mut writable = Map::from_iter([
        ("item".to_owned(), json!({"id": item_id})),
        ("quantity".to_owned(), json!(quantity)),
    ]);
    for key in ["id", "parent_id", "input"] {
        if let Some(value) = line_item.get(key) {
            writable.insert(key.to_owned(), value.clone());
        }
    }
    Ok(Value::Object(writable))
}

fn selected_payment(checkout: &Value) -> Option<&Value> {
    checkout
        .pointer("/payment/instruments")
        .and_then(Value::as_array)
        .and_then(|instruments| {
            instruments.iter().find(|instrument| {
                instrument.get("selected").and_then(Value::as_bool) == Some(true)
            })
        })
}

fn payment_selection_body(instrument_id: &str) -> Value {
    json!({"instruments": [{"id": instrument_id, "selected": true}]})
}

fn preserved_payment_body(payment: &Value) -> Result<Value> {
    let payment_id = payment.get("id").and_then(Value::as_str).ok_or_else(|| {
        GddyError::unexpected("checkout response contained a selected payment method without an ID")
            .into_cli_error()
    })?;
    let mut instrument = Map::new();
    for key in ["id", "selected", "handler_id", "type", "billing_address"] {
        if let Some(value) = payment.get(key) {
            instrument.insert(key.to_owned(), value.clone());
        }
    }
    instrument.insert("id".to_owned(), Value::String(payment_id.to_owned()));
    instrument.insert("selected".to_owned(), Value::Bool(true));
    Ok(json!({"instruments": Value::Array(vec![Value::Object(instrument)])}))
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
        .expect("checkout request is an object");
    let context = object.entry("context").or_insert_with(|| json!({}));
    let context = context
        .as_object_mut()
        .ok_or_else(|| GddyError::validation("context must be a JSON object").into_cli_error())?;
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
            "only one payment instrument may be specified for a checkout session",
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

pub(crate) fn no_saved_payment_method_action(
    checkout: &Value,
    account_url: &str,
) -> Option<cli_engine::NextAction> {
    checkout
        .pointer("/payment/instruments")
        .and_then(Value::as_array)
        .is_some_and(Vec::is_empty)
        .then(|| {
            next_action(
                "payment-methods add",
                format!(
                    "No saved payment method is available. Add one at {account_url}/payment-methods/add-payment, then retrieve this checkout session again."
                ),
            )
        })
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

    fn checkout() -> Value {
        json!({
            "line_items": [{
                "id": "line-1",
                "item": {"id": "product-1", "title": "Product"},
                "quantity": 2,
                "input": {"domain": "example.com"},
                "included_products": [{"id": "included-product"}],
                "totals": [{"type": "total", "amount": 100}]
            }],
            "context": {"currency": "USD", "language": "en"},
            "buyer": {"first_name": "Jane", "last_name": "Doe", "email": "jane@example.test"},
            "payment": {"instruments": [{"id": "payment-1", "selected": true, "description": "Visa"}]}
        })
    }

    #[test]
    fn update_rebuilds_full_writable_state_and_merges_explicit_changes() {
        let body = CheckoutInput {
            buyer_email: Some("updated@example.test".to_owned()),
            payment_instrument: Some("payment-2".to_owned()),
            ..CheckoutInput::default()
        }
        .update_body(&checkout())
        .expect("buyer and payment updates should be valid");

        assert_eq!(
            body,
            json!({
                "line_items": [{
                    "id": "line-1",
                    "item": {"id": "product-1"},
                    "quantity": 2,
                    "input": {"domain": "example.com"}
                }],
                "context": {"currency": "USD", "language": "en"},
                "buyer": {"first_name": "Jane", "last_name": "Doe", "email": "updated@example.test"},
                "payment": {"instruments": [{"id": "payment-2", "selected": true}]}
            })
        );
    }

    #[test]
    fn currency_update_preserves_state_and_uses_explicit_payment_selection() {
        let body = CheckoutInput {
            currency: Some("GBP".to_owned()),
            payment_instrument: Some("payment-2".to_owned()),
            ..CheckoutInput::default()
        }
        .update_body(&checkout())
        .expect("currency change with explicit payment should be valid");

        assert_eq!(body["context"]["currency"], "GBP");
        assert_eq!(body["payment"]["instruments"][0]["id"], "payment-2");
        assert_eq!(body["line_items"][0]["item"]["id"], "product-1");
    }

    #[test]
    fn update_replaces_items_and_retains_other_writable_state() {
        let body = CheckoutInput {
            items: vec!["product-2=3".to_owned()],
            ..CheckoutInput::default()
        }
        .update_body(&checkout())
        .expect("item replacement should be valid");
        assert_eq!(
            body["line_items"],
            json!([{"item": {"id": "product-2"}, "quantity": 3}])
        );
        assert_eq!(body["buyer"]["email"], "jane@example.test");
        assert_eq!(body["payment"]["instruments"][0]["id"], "payment-1");
    }

    #[test]
    fn update_rejects_no_changes_or_incomplete_checkout_responses() {
        assert!(CheckoutInput::default().update_body(&checkout()).is_err());
        assert!(
            CheckoutInput {
                buyer_email: Some("jane@example.test".to_owned()),
                ..CheckoutInput::default()
            }
            .update_body(&json!({}))
            .is_err()
        );
    }

    #[test]
    fn builds_structured_completion_with_one_payment_instrument() {
        let body = CheckoutInput {
            payment_instrument: Some("payment-1".to_owned()),
            ..CheckoutInput::default()
        }
        .completion_body()
        .expect("completion body should build");

        assert_eq!(
            body,
            json!({
                "payment": {"instruments": [{"id": "payment-1", "selected": true}]}
            })
        );
    }

    #[test]
    fn adds_payment_method_actions_for_resolved_environment_urls() {
        let empty_instruments = json!({"payment": {"instruments": []}});
        for account_url in [
            "https://account.godaddy.com",
            "https://account.test-godaddy.com",
            "https://account.dev-godaddy.com",
        ] {
            let action = no_saved_payment_method_action(&empty_instruments, account_url)
                .expect("empty list should require a payment method");
            assert_eq!(action.command, "gddy payment-methods add");
            assert!(
                action
                    .description
                    .contains(&format!("{account_url}/payment-methods/add-payment"))
            );
        }
        assert!(
            no_saved_payment_method_action(&json!({"payment": {}}), "https://account.godaddy.com")
                .is_none()
        );
    }

    #[test]
    fn reports_api_update_errors() {
        assert!(
            update_response(json!({
                "messages": [{"type": "error", "content": "invalid payment"}]
            }))
            .expect_err("API error should be reported")
            .to_string()
            .contains("invalid payment")
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
    fn follow_up_commands_rely_on_the_selected_environment() {
        assert_eq!(
            command_for_env("test", "order get order-1 --wait"),
            "shopping order get order-1 --wait"
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
