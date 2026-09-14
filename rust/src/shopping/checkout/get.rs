use cli_engine::{CommandResult, CommandSpec, NextActionParam, Result, RuntimeCommandSpec, Tier};
use serde_json::Value;

use crate::next_action::next_action;
use crate::shopping::SHOPPING_SCOPES;
use crate::shopping::common::{client_err, make_client};
use crate::shopping::human::{CHECKOUT_VIEW_ID, checkout_response};

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
        CommandSpec::from_args::<Args>("get", "Review an open checkout session")
            .with_long(
                "Review an open checkout session, including its items, available payment methods, and \
                 important links. Use the order ID returned after placing an order to review a completed \
                 purchase.",
            )
            .with_system("shopping")
            .with_tier(Tier::Read)
            .with_scopes(SHOPPING_SCOPES)
            .with_view_id(CHECKOUT_VIEW_ID),
        |ctx, args: Args| async move {
            let checkout = client_response(&ctx, &args.id).await?;
            let ready_for_complete =
                checkout.get("status").and_then(Value::as_str) == Some("ready_for_complete");
            let actions = if ready_for_complete {
                vec![
                    next_action(
                        "shopping checkout complete <checkout-id> --agree",
                        "Place an order after reviewing the checkout session and its terms",
                    )
                    .with_param("checkout-id", NextActionParam::value(args.id)),
                ]
            } else {
                Vec::new()
            };
            let output = if ctx.middleware.output_format == "human" {
                checkout_response(&checkout, args.show_all_payment_instruments)
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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::shopping::human::checkout_response;

    #[test]
    fn payment_instruments_default_to_five_and_can_show_all() {
        let checkout = json!({
            "payment": {"instruments": (1..=6)
                .map(|id| json!({"id": id.to_string(), "description": format!("Card {id}")}))
                .collect::<Vec<_>>()}
        });

        let default_response = checkout_response(&checkout, false);
        let all_response = checkout_response(&checkout, true);

        assert_eq!(
            default_response["available_payment_instruments"]
                .as_array()
                .map(Vec::len),
            Some(5)
        );
        assert_eq!(
            all_response["available_payment_instruments"]
                .as_array()
                .map(Vec::len),
            Some(6)
        );
        assert_eq!(default_response["has_more_payment_instruments"], true);
    }

    #[test]
    fn human_response_shows_cart_essentials_and_all_links() {
        let response = checkout_response(
            &json!({
                "id": "checkout-1",
                "status": "ready_for_complete",
                "buyer": {"first_name": "Jane", "email": "jane@example.test"},
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
        );

        assert_eq!(response["id"], "checkout-1");
        assert_eq!(response["items"][0]["title"], "Web Hosting Economy");
        assert_eq!(
            response["selected_payment"],
            "CREDIT_CARD/VISA 1111 (ID: payment-1)"
        );
        assert_eq!(response["total"], "USD 83.88");
        assert_eq!(response["buyer"]["first_name"], "Jane");
        assert_eq!(response["buyer"]["email"], "jane@example.test");
        assert_eq!(response["links"][0]["title"], "terms of service");
        assert_eq!(response["links"][1]["title"], "Help centre");
        assert!(!response.to_string().contains("do not render"));
    }
}
