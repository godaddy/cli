use cli_engine::{CommandResult, CommandSpec, ModuleContext, Result, RuntimeCommandSpec, Tier};
use serde_json::{Value, json};

use crate::next_action::next_action;
use crate::shopping::common::{client_err, make_client};
use crate::shopping::money;
use crate::shopping::{SHOPPING_SCOPES, command_for_env};

pub(super) const HUMAN_VIEW_ID: &str = "shopping-checkout-get";

pub(crate) fn register_human_view(ctx: &mut ModuleContext<'_>) {
    ctx.middleware_mut()
        .human_views
        .register_func(HUMAN_VIEW_ID, render_human);
}

#[derive(Debug, Clone, clap::Args)]
struct Args {
    /// Checkout session ID.
    #[arg(value_name = "CHECKOUT_ID")]
    id: String,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<Args, _, _, _>(
        CommandSpec::from_args::<Args>("get", "Get an open Shopping checkout session")
            .with_long(
                "Get an open checkout session. Do not use after completion: completed checkouts \
                 cannot be retrieved through this command. Use `shopping order get` with the order \
                 ID returned by completion instead.",
            )
            .with_system("shopping")
            .with_tier(Tier::Read)
            .with_scopes(SHOPPING_SCOPES)
            .with_view_id(HUMAN_VIEW_ID),
        |ctx, args: Args| async move {
            let checkout = client_response(&ctx, &args.id).await?;
            let ready_for_complete =
                checkout.get("status").and_then(Value::as_str) == Some("ready_for_complete");
            let output = if ctx.middleware.output_format == "human" {
                human_response(&checkout)
            } else {
                checkout
            };
            let mut result = CommandResult::new(output);
            if ready_for_complete {
                result = result.with_next_actions(vec![next_action(
                    command_for_env(
                        &ctx.middleware.env,
                        format!(
                            "checkout complete {} --file complete-checkout.json",
                            args.id
                        ),
                    ),
                    "Complete this checkout after reviewing its selected payment method",
                )]);
            }
            Ok(result)
        },
    )
}

async fn client_response(ctx: &cli_engine::CommandContext, id: &str) -> Result<Value> {
    let client = make_client(ctx).await?;
    client.get_checkout(id).await.map_err(client_err)
}

pub(super) fn human_response(checkout: &Value) -> Value {
    let line_items = checkout
        .get("line_items")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|item| {
                    json!({
                        "quantity": item.get("quantity").and_then(Value::as_u64).unwrap_or(1),
                        "title": item.pointer("/item/title").and_then(Value::as_str).unwrap_or("Unknown item"),
                        "included": item.get("included_products").and_then(Value::as_array).map(|products| products.iter().filter_map(|product| product.get("title").and_then(Value::as_str)).collect::<Vec<_>>()).unwrap_or_default(),
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    json!({
        "id": checkout.get("id").and_then(Value::as_str).unwrap_or_default(),
        "status": checkout.get("status").and_then(Value::as_str).unwrap_or_default(),
        "items": line_items,
        "currency": checkout.get("currency").and_then(Value::as_str).unwrap_or_default(),
        "totals": checkout.get("totals").cloned().unwrap_or_else(|| json!([])),
        "selected_payment": selected_payment(checkout),
    })
}

fn selected_payment(checkout: &Value) -> String {
    let Some(instrument) = checkout
        .pointer("/payment/instruments")
        .and_then(Value::as_array)
        .and_then(|instruments| {
            instruments.iter().find(|instrument| {
                instrument.get("selected").and_then(Value::as_bool) == Some(true)
            })
        })
    else {
        return "No payment method selected".to_owned();
    };
    let description = instrument
        .get("rich_text_description")
        .and_then(Value::as_str)
        .unwrap_or("Selected payment method");
    match instrument.get("id").and_then(Value::as_str) {
        Some(id) => format!("{description} (ID: {id})"),
        None => description.to_owned(),
    }
}

fn render_human(checkout: &Value) -> String {
    let mut output = format!(
        "Checkout: {}\nStatus: {}\n",
        checkout
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        checkout
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    );
    output.push_str("\nItems:\n");
    let items = checkout
        .get("items")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    if items.is_empty() {
        output.push_str("- None\n");
    }
    for item in items {
        output.push_str(&format!(
            "- {} × {}\n",
            item.get("quantity").and_then(Value::as_u64).unwrap_or(1),
            item.get("title")
                .and_then(Value::as_str)
                .unwrap_or("Unknown item"),
        ));
        for included in item
            .get("included")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(included) = included.as_str() {
                output.push_str(&format!("  Includes: {included}\n"));
            }
        }
    }
    output.push_str(&format!(
        "\nSelected payment: {}\nTotals:\n",
        checkout
            .get("selected_payment")
            .and_then(Value::as_str)
            .unwrap_or("No payment method selected"),
    ));
    let totals = checkout
        .get("totals")
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
            checkout
                .get("currency")
                .and_then(Value::as_str)
                .map_or_else(
                    || amount.to_string(),
                    |currency| money::format_amount(amount, currency)
                ),
        ));
    }
    output.push_str("\nCompletion places a real order. Review this checkout before continuing.\n");
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_view_masks_checkout_to_purchase_essentials() {
        let output = render_human(&human_response(&json!({
            "id": "checkout-1",
            "status": "ready_for_complete",
            "line_items": [{
                "quantity": 1,
                "item": {"title": "Web Hosting Economy"},
                "included_products": [{"title": "Standard SSL"}]
            }],
            "payment": {"instruments": [{
                "selected": true,
                "rich_text_description": "CREDIT_CARD/VISA 1111",
                "billing_address": {"street_address": "do not render"}
            }]},
            "totals": [{"display_text": "Total", "amount": 8388}]
        })));

        assert!(output.contains("Web Hosting Economy"));
        assert!(output.contains("CREDIT_CARD/VISA 1111"));
        assert!(output.contains("Completion places a real order"));
        assert!(!output.contains("do not render"));
    }
}
