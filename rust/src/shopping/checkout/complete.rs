use cli_engine::{
    CommandResult, CommandSpec, ModuleContext, NextActionParam, RuntimeCommandSpec, Tier,
};
use serde_json::{Value, json};

use crate::next_action::next_action;
use crate::shopping::SHOPPING_SCOPES;
use crate::shopping::client::ClientError;
use crate::shopping::common::{
    CheckoutInput, has_conflicting_checkout_id, make_client, read_json,
    reject_mixed_checkout_input, require_selected_payment_instrument,
};

#[derive(Debug, Clone, clap::Args)]
struct Args {
    /// Cart ID.
    #[arg(value_name = "CHECKOUT_ID")]
    id: String,

    /// Saved payment method ID to use for this purchase.
    #[arg(long, value_name = "INSTRUMENT_ID")]
    payment_instrument: Option<String>,

    /// Acknowledge the cart's terms and other important links.
    #[arg(long)]
    agree: bool,

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

// The transport key is an implementation detail, not customer-facing output.
fn human_response(completion: &Value) -> Value {
    json!({
        "cart_id": completion.get("id").and_then(Value::as_str).unwrap_or_default(),
        "status": completion.get("status").and_then(Value::as_str).unwrap_or_default(),
        "order_id": completion.pointer("/order/id").and_then(Value::as_str),
        "order_permalink": completion.pointer("/order/permalink_url").and_then(Value::as_str),
        "total": crate::shopping::money::format_total(
            completion.get("totals"),
            completion.get("currency").and_then(Value::as_str),
        ),
    })
}

fn render_human(completion: &Value) -> String {
    if let Some(action) = completion.get("action").and_then(Value::as_str) {
        return format!(
            "{action}\nCart: {}\n",
            completion
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        );
    }
    let mut output = format!(
        "Cart: {}\nStatus: {}\n",
        completion
            .get("cart_id")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        completion
            .get("status")
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
    output
}

fn completion_error(error: ClientError) -> cli_engine::CliCoreError {
    crate::error::GddyError::from(error).into_cli_error()
}

fn public_completion_body(body: &Value) -> Value {
    let mut body = body.clone();
    body.as_object_mut()
        .expect("completion body is an object")
        .remove("idempotency_key");
    body
}

fn agreement_gate(agree: bool) -> cli_engine::Result<()> {
    if agree {
        return Ok(());
    }
    Err(crate::error::GddyError::validation(
        "placing an order requires acknowledging the cart's terms and important links",
    )
    .with_fix(
        "Review the cart with `shopping checkout get <checkout-id>`, then re-run with --agree.",
    )
    .into_cli_error())
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<Args, _, _, _>(
        CommandSpec::from_args::<Args>("complete", "Place an order with the contents of your cart")
            .with_long(
                "Place an order with a selected saved payment method. Review the cart and its links \
                 first, then use --agree to acknowledge them.",
            )
            .with_system("shopping")
            .with_tier(Tier::Mutate)
            .mutates(true)
            .handles_dry_run(true)
            .with_scopes(SHOPPING_SCOPES)
            .auth_optional()
            .with_view_id(HUMAN_VIEW_ID),
        |ctx, args: Args| async move {
            agreement_gate(args.agree)?;
            let payment_instrument = args.payment_instrument;
            let input = CheckoutInput {
                payment_instrument,
                ..CheckoutInput::default()
            };
            reject_mixed_checkout_input(args.body.as_deref(), args.file.as_deref(), input.is_present())?;
            let mut body = if args.body.is_some() || args.file.is_some() {
                read_json(args.body.as_deref(), args.file.as_deref(), "object")?
            } else {
                input.completion_body()?
            };
            if has_conflicting_checkout_id(&body, &args.id) {
                return Err(crate::error::GddyError::validation(
                    "cart ID in request body conflicts with CHECKOUT_ID",
                )
                .into_cli_error());
            }
            require_selected_payment_instrument(&body)?;
            let idempotency_key = uuid::Uuid::new_v4().to_string();
            body.as_object_mut()
                .expect("completion body is an object")
                .insert(
                    "idempotency_key".to_owned(),
                    Value::String(idempotency_key.clone()),
                );
            if ctx.dry_run() {
                return Ok(CommandResult::new(json!({
                    "action": "dry-run: would place order",
                    "id": args.id,
                    "body": public_completion_body(&body),
                }))
                .with_dry_run());
            }

            let client = make_client(&ctx).await?;
            let completion = client
                .complete_checkout(&args.id, body, &idempotency_key)
                .await
                .map_err(completion_error)?;
            let order_id = completion.pointer("/order/id").and_then(Value::as_str);
            let actions = order_id.map_or_else(Vec::new, |order_id| {
                vec![
                    next_action(
                        "shopping order get <order-id> --wait",
                        "Review the order after it becomes visible",
                    )
                    .with_param("order-id", NextActionParam::value(order_id)),
                ]
            });
            let output = if ctx.middleware.output_format == "human" {
                human_response(&completion)
            } else {
                completion
            };
            Ok(CommandResult::new(output).with_next_actions(actions))
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agreement_gate_requires_agree() {
        let error = agreement_gate(false).expect_err("must require --agree");
        assert!(error.to_string().contains("acknowledging"));
        assert!(agreement_gate(true).is_ok());
    }

    #[test]
    fn human_view_shows_completion_total_only_with_currency() {
        let output = render_human(&human_response(&json!({
            "id": "checkout-1",
            "status": "completed",
            "currency": "GBP",
            "totals": [
                {"type": "subtotal", "amount": 4788},
                {"type": "tax", "amount": 0},
                {"type": "total", "amount": 4788}
            ]
        })));

        assert!(output.contains("Total: GBP 47.88"));
        assert!(!output.contains("Subtotal:"));
        assert!(!output.contains("Tax:"));
    }

    #[test]
    fn human_view_omits_completion_total_without_currency() {
        let output = render_human(&human_response(&json!({
            "id": "checkout-1",
            "status": "completed",
            "totals": [{"type": "total", "amount": 4788}]
        })));

        assert!(!output.contains("Total:"));
        assert!(!output.contains("4788"));
    }
}
