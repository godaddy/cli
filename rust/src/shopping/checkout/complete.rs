use cli_engine::{
    CommandResult, CommandSpec, ModuleContext, NextActionParam, RuntimeCommandSpec, Tier,
};
use serde_json::{Value, json};

use crate::next_action::{human_next_steps, next_action};
use crate::shopping::client::ClientError;
use crate::shopping::common::{
    CheckoutInput, ensure_completion_idempotency_key, has_conflicting_checkout_id, make_client,
    read_json, reject_mixed_checkout_input, require_selected_payment_instrument,
};
use crate::shopping::{SHOPPING_SCOPES, command_for_env};

#[derive(Debug, Clone, clap::Args)]
struct Args {
    /// Checkout session ID.
    #[arg(value_name = "CHECKOUT_ID")]
    id: String,

    /// Saved payment instrument ID to use for this purchase.
    #[arg(long, value_name = "INSTRUMENT_ID")]
    payment_instrument: Option<String>,

    /// Stable key for this single intended purchase. A UUID is generated when omitted.
    #[arg(long, value_name = "KEY")]
    idempotency_key: Option<String>,

    /// Completion request as raw JSON for advanced payment or billing-address fields.
    #[arg(long, value_name = "JSON")]
    body: Option<String>,

    /// Path to a JSON completion request. Takes precedence over --body.
    #[arg(long, value_name = "PATH")]
    file: Option<String>,
}

const HUMAN_VIEW_ID: &str = "shopping-checkout-complete";

pub(crate) fn register_human_view(ctx: &mut ModuleContext<'_>) {
    ctx.middleware_mut()
        .human_views
        .register_func(HUMAN_VIEW_ID, render_human);
}

fn human_response(
    completion: &Value,
    idempotency_key: &str,
    actions: &[cli_engine::NextAction],
) -> Value {
    json!({
        "checkout_id": completion.get("id").and_then(Value::as_str).unwrap_or_default(),
        "status": completion.get("status").and_then(Value::as_str).unwrap_or_default(),
        "order_id": completion.pointer("/order/id").and_then(Value::as_str),
        "order_permalink": completion.pointer("/order/permalink_url").and_then(Value::as_str),
        "total": crate::shopping::money::format_total(
            completion.get("totals"),
            completion.get("currency").and_then(Value::as_str),
        ),
        "idempotency_key": idempotency_key,
        "next_steps": human_next_steps(actions),
    })
}

fn render_human(completion: &Value) -> String {
    if let Some(action) = completion.get("action").and_then(Value::as_str) {
        let idempotency_key = completion
            .get("idempotency_key")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let body = completion.get("body").cloned().unwrap_or(Value::Null);
        return format!(
            "{}\nCheckout: {}\nIdempotency key: {idempotency_key}\nRequest:\n{}\n",
            action,
            completion
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            serde_json::to_string_pretty(&body).unwrap_or_else(|_| body.to_string()),
        );
    }
    let mut output = format!(
        "Checkout: {}\nStatus: {}\nIdempotency key: {}\n",
        completion
            .get("checkout_id")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        completion
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        completion
            .get("idempotency_key")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    );
    if let Some(order_id) = completion.get("order_id").and_then(Value::as_str) {
        output.push_str(&format!("Order: {order_id}\n"));
    }
    if let Some(permalink) = completion.get("order_permalink").and_then(Value::as_str) {
        output.push_str(&format!("View order: {permalink}\n"));
    }
    if let Some(total) = completion.get("total").and_then(Value::as_str) {
        output.push_str(&format!("Total: {total}\n"));
    }
    output.push_str("\nKeep this idempotency key. Do not retry a completion unless you first confirm its outcome.\n");
    crate::shopping::checkout::get::render_next_steps(&mut output, completion);
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_view_shows_completion_total_only_with_currency() {
        let output = render_human(&human_response(
            &json!({
                "id": "checkout-1",
                "status": "completed",
                "currency": "GBP",
                "totals": [
                    {"type": "subtotal", "amount": 4788},
                    {"type": "tax", "amount": 0},
                    {"type": "total", "amount": 4788}
                ]
            }),
            "idempotency-key",
            &[],
        ));

        assert!(output.contains("Total: GBP 47.88"));
        assert!(!output.contains("Subtotal:"));
        assert!(!output.contains("Tax:"));
    }

    #[test]
    fn human_view_omits_completion_total_without_currency() {
        let output = render_human(&human_response(
            &json!({
                "id": "checkout-1",
                "status": "completed",
                "totals": [{"type": "total", "amount": 4788}]
            }),
            "idempotency-key",
            &[],
        ));

        assert!(!output.contains("Total:"));
        assert!(!output.contains("4788"));
    }
}

fn completion_error(error: ClientError, idempotency_key: &str) -> cli_engine::CliCoreError {
    crate::error::GddyError::from(error)
        .with_fix(format!(
            "Completion may have reached Shopping. Do not retry automatically. Reuse idempotency_key \
             {idempotency_key:?} only for the same intended purchase after confirming its outcome."
        ))
        .into_cli_error()
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<Args, _, _, _>(
        CommandSpec::from_args::<Args>("complete", "Complete a Shopping checkout and place an order")
            .with_long(
                "Places a real order. Use --payment-instrument for one saved payment instrument, \
                 or --body/--file for advanced payment or billing-address fields. Use --idempotency-key \
                 to control retries, or omit it to let gddy generate and return one. The CLI never retries \
                 completion automatically. Read the resulting order with `shopping order get <order-id> --wait`.",
            )
            .with_system("shopping")
            .with_tier(Tier::Mutate)
            .mutates(true)
            .handles_dry_run(true)
            .with_scopes(SHOPPING_SCOPES)
            .auth_optional()
            .with_view_id(HUMAN_VIEW_ID),
        |ctx, args: Args| async move {
            let input = CheckoutInput {
                payment_instrument: args.payment_instrument,
                ..CheckoutInput::default()
            };
            reject_mixed_checkout_input(args.body.as_deref(), args.file.as_deref(), input.is_present())?;
            if args.idempotency_key.as_deref().is_some_and(|key| key.trim().is_empty()) {
                return Err(crate::error::GddyError::validation(
                    "--idempotency-key must be non-empty when supplied",
                )
                .into_cli_error());
            }
            let mut body = if args.body.is_some() || args.file.is_some() {
                let mut body = read_json(args.body.as_deref(), args.file.as_deref(), "object")?;
                if let Some(idempotency_key) = args.idempotency_key {
                    body.as_object_mut()
                        .expect("read_json validates the completion request is an object")
                        .insert("idempotency_key".to_owned(), json!(idempotency_key));
                }
                body
            } else {
                input.completion_body(args.idempotency_key.as_deref())?
            };
            if has_conflicting_checkout_id(&body, &args.id) {
                return Err(crate::error::GddyError::validation(
                    "checkout ID in request body conflicts with CHECKOUT_ID",
                )
                .into_cli_error());
            }
            require_selected_payment_instrument(&body)?;
            let idempotency_key = ensure_completion_idempotency_key(&mut body)?;
            if ctx.dry_run() {
                return Ok(CommandResult::new(json!({
                    "action": "dry-run: would complete checkout",
                    "id": args.id,
                    "idempotency_key": idempotency_key,
                    "body": body,
                }))
                .with_dry_run());
            }

            let client = make_client(&ctx).await?;
            let completion = client.complete_checkout(&args.id, body).await.map_err(|error| {
                completion_error(error, &idempotency_key)
            })?;
            let order_id = completion
                .pointer("/order/id")
                .and_then(Value::as_str)
                .map(str::to_owned);
            let actions = order_id.map_or_else(Vec::new, |order_id| {
                vec![
                    next_action(
                        command_for_env(&ctx.middleware.env, "order get <order-id> --wait"),
                        "Read the completed order after it becomes visible",
                    )
                    .with_param("order-id", NextActionParam::value(order_id)),
                ]
            });
            let output = if ctx.middleware.output_format == "human" {
                human_response(&completion, &idempotency_key, &actions)
            } else {
                completion
            };
            Ok(CommandResult::new(output).with_next_actions(actions))
        },
    )
}
