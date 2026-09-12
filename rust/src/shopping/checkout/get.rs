use cli_engine::{
    CommandResult, CommandSpec, ModuleContext, NextActionParam, Result, RuntimeCommandSpec, Tier,
};
use serde_json::{Value, json};

use crate::next_action::next_action;
use crate::shopping::SHOPPING_SCOPES;
use crate::shopping::common::{client_err, make_client};
use crate::shopping::money;

pub(super) const HUMAN_VIEW_ID: &str = "shopping-checkout-get";

pub(crate) fn register_human_view(ctx: &mut ModuleContext<'_>) {
    ctx.middleware_mut()
        .human_views
        .register_func(HUMAN_VIEW_ID, render_human);
}

#[derive(Debug, Clone, clap::Args)]
struct Args {
    /// Cart ID.
    #[arg(value_name = "CHECKOUT_ID")]
    id: String,

    /// Show every available saved payment method instead of the first five.
    #[arg(long)]
    show_all_payment_instruments: bool,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<Args, _, _, _>(
        CommandSpec::from_args::<Args>("get", "Review an open cart")
            .with_long(
                "Review an open cart, including its items, available payment methods, and important links. \
                 Use the order ID returned after placing an order to review a completed purchase.",
            )
            .with_system("shopping")
            .with_tier(Tier::Read)
            .with_scopes(SHOPPING_SCOPES)
            .with_view_id(HUMAN_VIEW_ID),
        |ctx, args: Args| async move {
            let checkout = client_response(&ctx, &args.id).await?;
            let ready_for_complete =
                checkout.get("status").and_then(Value::as_str) == Some("ready_for_complete");
            let actions = if ready_for_complete {
                let payment_instrument = selected_payment_id(&checkout).unwrap_or("<payment-instrument>");
                vec![
                    next_action(
                        "shopping checkout complete <checkout-id> --payment-instrument <payment-instrument> --agree",
                        "Place an order after reviewing the cart and its terms",
                    )
                    .with_param("checkout-id", NextActionParam::value(args.id))
                    .with_param(
                        "payment-instrument",
                        NextActionParam::value(payment_instrument),
                    ),
                ]
            } else {
                Vec::new()
            };
            let output = if ctx.middleware.output_format == "human" {
                human_response(&checkout, args.show_all_payment_instruments)
            } else {
                checkout
            };
            Ok(CommandResult::new(output).with_next_actions(actions))
        },
    )
}

pub(super) async fn client_response(ctx: &cli_engine::CommandContext, id: &str) -> Result<Value> {
    let client = make_client(ctx).await?;
    client.get_checkout(id).await.map_err(client_err)
}

pub(super) fn human_response(checkout: &Value, show_all_payment_instruments: bool) -> Value {
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
        "total": money::format_total(
            checkout.get("totals"),
            checkout.get("currency").and_then(Value::as_str),
        ),
        "selected_payment": selected_payment(checkout),
        "available_payment_instruments": available_payment_instruments(checkout, show_all_payment_instruments),
        "has_more_payment_instruments": !show_all_payment_instruments && available_payment_instrument_count(checkout) > PAYMENT_INSTRUMENT_LIMIT,
        "links": checkout_links(checkout),
    })
}

const PAYMENT_INSTRUMENT_LIMIT: usize = 5;

fn available_payment_instruments(checkout: &Value, show_all: bool) -> Vec<Value> {
    checkout
        .pointer("/payment/instruments")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|instrument| {
            let id = instrument.get("id").and_then(Value::as_str)?;
            Some(json!({
                "id": id,
                "description": payment_instrument_description(instrument),
                "selected": instrument.get("selected").and_then(Value::as_bool).unwrap_or(false),
            }))
        })
        .take(if show_all {
            usize::MAX
        } else {
            PAYMENT_INSTRUMENT_LIMIT
        })
        .collect()
}

fn available_payment_instrument_count(checkout: &Value) -> usize {
    checkout
        .pointer("/payment/instruments")
        .and_then(Value::as_array)
        .map_or(0, Vec::len)
}

fn payment_instrument_description(instrument: &Value) -> &str {
    instrument
        .get("rich_text_description")
        .or_else(|| instrument.get("description"))
        .and_then(Value::as_str)
        .unwrap_or("Saved payment method")
}

fn selected_payment(checkout: &Value) -> String {
    let Some(instrument) = selected_payment_instrument(checkout) else {
        return "No payment method selected".to_owned();
    };
    let description = payment_instrument_description(instrument);
    match instrument.get("id").and_then(Value::as_str) {
        Some(id) => format!("{description} (ID: {id})"),
        None => description.to_owned(),
    }
}

fn selected_payment_instrument(checkout: &Value) -> Option<&Value> {
    checkout
        .pointer("/payment/instruments")
        .and_then(Value::as_array)
        .and_then(|instruments| {
            instruments.iter().find(|instrument| {
                instrument.get("selected").and_then(Value::as_bool) == Some(true)
            })
        })
}

pub(super) fn selected_payment_id(checkout: &Value) -> Option<&str> {
    selected_payment_instrument(checkout)
        .and_then(|instrument| instrument.get("id"))?
        .as_str()
}

fn checkout_links(checkout: &Value) -> Vec<Value> {
    checkout
        .get("links")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|link| {
            let url = link.get("url").and_then(Value::as_str)?;
            let link_type = link.get("type").and_then(Value::as_str).unwrap_or("link");
            let title = link
                .get("title")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or_else(|| link_type.replace('_', " "));
            Some(json!({"title": title, "url": url, "type": link_type}))
        })
        .collect()
}

fn render_human(cart: &Value) -> String {
    if let Some(action) = cart.get("action").and_then(Value::as_str) {
        let body = cart.get("body").cloned().unwrap_or(Value::Null);
        return format!(
            "{action}\nRequest:\n{}\n",
            serde_json::to_string_pretty(&body).unwrap_or_else(|_| body.to_string()),
        );
    }
    let mut output = format!(
        "Cart: {}\nStatus: {}\n",
        cart.get("id").and_then(Value::as_str).unwrap_or_default(),
        cart.get("status")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    );
    output.push_str("\nItems:\n");
    let items = cart
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
        "\nSelected payment: {}\n",
        cart.get("selected_payment")
            .and_then(Value::as_str)
            .unwrap_or("No payment method selected"),
    ));
    render_available_payment_instruments(&mut output, cart);
    if let Some(total) = cart.get("total").and_then(Value::as_str) {
        output.push_str(&format!("\nTotal: {total}\n"));
    }
    render_links(&mut output, cart);
    output.push_str("\nReview this cart and its links before placing an order.\n");
    output
}

fn render_available_payment_instruments(output: &mut String, cart: &Value) {
    let instruments = cart
        .get("available_payment_instruments")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    if instruments.is_empty() {
        return;
    }
    output.push_str("\nAvailable payment methods:\n");
    for instrument in instruments {
        let selected = if instrument
            .get("selected")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            " (selected)"
        } else {
            ""
        };
        output.push_str(&format!(
            "- {} (ID: {}){selected}\n",
            instrument
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("Saved payment method"),
            instrument
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        ));
    }
    if cart
        .get("has_more_payment_instruments")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        output.push_str("Use --show-all-payment-instruments to show every saved payment method.\n");
    }
}

fn render_links(output: &mut String, cart: &Value) {
    let links = cart
        .get("links")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    if links.is_empty() {
        return;
    }
    output.push_str("\nImportant links:\n");
    for link in links {
        output.push_str(&format!(
            "- {}: {}\n",
            link.get("title").and_then(Value::as_str).unwrap_or("Link"),
            link.get("url").and_then(Value::as_str).unwrap_or_default(),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payment_instruments_default_to_five_and_can_show_all() {
        let checkout = json!({
            "payment": {"instruments": (1..=6)
                .map(|id| json!({"id": id.to_string(), "description": format!("Card {id}")}))
                .collect::<Vec<_>>()}
        });

        let default_instruments = available_payment_instruments(&checkout, false);
        let all_instruments = available_payment_instruments(&checkout, true);

        assert_eq!(default_instruments.len(), 5);
        assert_eq!(all_instruments.len(), 6);
        let output = render_human(&human_response(&checkout, false));
        assert!(output.contains("--show-all-payment-instruments"));
    }

    #[test]
    fn human_view_shows_cart_essentials_and_all_links() {
        let output = render_human(&human_response(
            &json!({
                "id": "checkout-1",
                "status": "ready_for_complete",
                "line_items": [{
                    "quantity": 1,
                    "item": {"title": "Web Hosting Economy"},
                    "included_products": [{"title": "Standard SSL"}]
                }],
                "totals": [{"type": "total", "amount": 8388}],
                "currency": "USD",
                "links": [
                    {"type": "terms_of_service", "url": "https://example.test/terms"},
                    {"type": "faq", "title": "Help centre", "url": "https://example.test/help"}
                ],
                "payment": {"instruments": [{
                    "id": "payment-1",
                    "selected": true,
                    "rich_text_description": "CREDIT_CARD/VISA 1111",
                    "billing_address": {"street_address": "do not render"}
                }]}
            }),
            false,
        ));

        assert!(output.contains("Cart: checkout-1"));
        assert!(output.contains("Web Hosting Economy"));
        assert!(output.contains("CREDIT_CARD/VISA 1111"));
        assert!(output.contains("Available payment methods:"));
        assert!(output.contains("Total: USD 83.88"));
        assert!(output.contains("terms of service: https://example.test/terms"));
        assert!(output.contains("Help centre: https://example.test/help"));
        assert!(!output.contains("do not render"));
    }
}
