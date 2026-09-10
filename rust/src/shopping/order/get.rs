use cli_engine::{CommandResult, CommandSpec, ModuleContext, RuntimeCommandSpec, Tier};
use serde_json::{Value, json};

use crate::output_schema::output_schema;
use crate::shopping::SHOPPING_SCOPES;
use crate::shopping::common::{client_err, make_client, wait_duration, wait_for_order};
use crate::shopping::money;

output_schema!(OrderOutput {
    "ucp": "object";
    "id": "string";
    "checkout_id": "string";
    "line_items": "[]object";
    "totals": "[]object";
});

#[derive(Debug, Clone, clap::Args)]
struct Args {
    /// Completed order ID returned by checkout completion.
    #[arg(value_name = "ORDER_ID")]
    id: String,

    /// Poll until the new order becomes available.
    #[arg(long)]
    wait: bool,

    /// Maximum seconds to wait for order visibility (1-60, default 15).
    #[arg(long, value_name = "SECONDS", requires = "wait")]
    wait_timeout: Option<u8>,
}

const HUMAN_VIEW_ID: &str = "shopping-order-get";

pub(crate) fn register_human_view(ctx: &mut ModuleContext<'_>) {
    ctx.middleware_mut()
        .human_views
        .register_func(HUMAN_VIEW_ID, render_human);
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<Args, _, _, _>(
        CommandSpec::from_args::<Args>("get", "Get a completed Shopping order")
            .with_long(
                "Read a completed order. New orders usually become available within 3-10 seconds; \
                 use --wait to poll for up to 15 seconds by default.",
            )
            .with_system("shopping")
            .with_tier(Tier::Read)
            .with_scopes(SHOPPING_SCOPES)
            .with_output_schema::<OrderOutput>()
            .with_view_id(HUMAN_VIEW_ID),
        |ctx, args: Args| async move {
            let client = make_client(&ctx).await?;
            let order = if args.wait {
                let (order, _) = wait_for_order(
                    &client,
                    &args.id,
                    wait_duration(args.wait_timeout)?,
                    &ctx.middleware.env,
                )
                .await?;
                order
            } else {
                client.get_order(&args.id).await.map_err(client_err)?
            };
            let output = if ctx.middleware.output_format == "human" {
                human_response(&order)
            } else {
                order
            };
            Ok(CommandResult::new(output))
        },
    )
}

fn human_response(order: &Value) -> Value {
    json!({
        "id": order.get("id").and_then(Value::as_str).unwrap_or_default(),
        "permalink_url": order.get("permalink_url").and_then(Value::as_str),
        "line_items": order.get("line_items").cloned().unwrap_or_else(|| json!([])),
        "totals": order.get("totals").cloned().unwrap_or_else(|| json!([])),
        "currency": order.get("currency").and_then(Value::as_str),
        "fulfillment": order.get("fulfillment").cloned().unwrap_or_else(|| json!({})),
    })
}

fn render_human(order: &Value) -> String {
    let mut output = format!(
        "Order: {}\n",
        order.get("id").and_then(Value::as_str).unwrap_or_default(),
    );
    if let Some(permalink) = order.get("permalink_url").and_then(Value::as_str) {
        output.push_str(&format!("View order: {permalink}\n"));
    }
    output.push_str("\nItems:\n");
    let items = order
        .get("line_items")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    if items.is_empty() {
        output.push_str("- None\n");
    }
    for item in items {
        let quantity = item
            .pointer("/quantity/total")
            .or_else(|| item.get("quantity"))
            .and_then(Value::as_u64)
            .unwrap_or(1);
        let title = item
            .pointer("/item/title")
            .and_then(Value::as_str)
            .unwrap_or("Unknown item");
        let status = item
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        output.push_str(&format!("- {quantity} × {title} · {status}\n"));
    }
    if order.get("currency").and_then(Value::as_str).is_some() {
        output.push_str("\nTotals:\n");
        render_totals(&mut output, order.get("totals"), order.get("currency"));
    }
    render_fulfillment(&mut output, order.get("fulfillment"));
    output
}

fn render_totals(output: &mut String, totals: Option<&Value>, currency: Option<&Value>) {
    let Some(currency) = currency.and_then(Value::as_str) else {
        return;
    };
    let totals = totals
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    if totals.is_empty() {
        output.push_str("- None\n");
    }
    for total in totals {
        let label = total
            .get("display_text")
            .or_else(|| total.get("type"))
            .and_then(Value::as_str)
            .unwrap_or("Total");
        let amount = total
            .get("amount")
            .and_then(Value::as_i64)
            .unwrap_or_default();
        output.push_str(&format!(
            "- {label}: {}\n",
            money::format_amount(amount, currency)
        ));
    }
}

fn render_fulfillment(output: &mut String, fulfillment: Option<&Value>) {
    let Some(fulfillment) = fulfillment.and_then(Value::as_object) else {
        return;
    };
    let expectations = fulfillment
        .get("expectations")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    if expectations.is_empty() {
        return;
    }
    output.push_str("\nFulfillment:\n");
    for expectation in expectations {
        if let Some(status) = expectation.get("status").and_then(Value::as_str) {
            output.push_str(&format!("- {status}\n"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_view_shows_order_essentials_without_ucp_metadata() {
        let output = render_human(&human_response(&json!({
            "id": "order-1",
            "checkout_id": "checkout-1",
            "permalink_url": "https://example.test/order-1",
            "currency": "USD",
            "line_items": [{"item": {"title": "Web Hosting Economy"}, "quantity": {"total": 1}, "status": "fulfilled"}],
            "totals": [{"display_text": "Total", "amount": 8388}],
            "ucp": {"do_not_render": true}
        })));

        assert!(output.contains("Order: order-1"));
        assert!(output.contains("Web Hosting Economy · fulfilled"));
        assert!(output.contains("USD 83.88"));
        assert!(!output.contains("Checkout:"));
        assert!(!output.contains("do_not_render"));
    }

    #[test]
    fn human_view_omits_totals_without_a_currency() {
        let output = render_human(&human_response(&json!({
            "id": "order-1",
            "totals": [{"display_text": "Total", "amount": 8388}]
        })));

        assert!(!output.contains("Totals:"));
        assert!(!output.contains("8388"));
    }
}
