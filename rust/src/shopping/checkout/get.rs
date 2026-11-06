use cli_engine::{
    CommandResult, CommandSpec, NextAction, NextActionParam, Result, RuntimeCommandSpec, Tier,
};
use serde_json::Value;

use super::create::agreement_review_description;
use crate::next_action::next_action;
use crate::shopping::SHOPPING_PURCHASE_SCOPES;
use crate::shopping::client::Checkout;
use crate::shopping::common::{
    client_err, make_client, no_saved_payment_method_action, reject_response_errors,
    selected_payment_id,
};
use crate::shopping::human::{CHECKOUT_VIEW_ID, checkout_response};

fn encode_checkout(checkout: &Checkout) -> Result<Value> {
    serde_json::to_value(checkout).map_err(|error| {
        crate::error::GddyError::unexpected(format!("failed to encode checkout response: {error}"))
            .into_cli_error()
    })
}

/// Suggests the step that follows from the checkout's current state. Payment
/// guidance applies to a checkout that is still being set up (`incomplete`) or
/// is ready to place (`ready_for_complete`); completion is only suggested for
/// the latter, with a selected payment method, and never with `--agree`: the
/// customer (or the AI assistant acting for them) adds it after the review this
/// command exists for.
fn next_actions(
    checkout: &Checkout,
    input_id: &str,
    account_url: &str,
    show_agent_note: bool,
) -> Vec<NextAction> {
    let checkout_id = checkout.id.clone().unwrap_or_else(|| input_id.to_owned());
    match checkout.status.as_deref() {
        Some("completed") => match checkout.order.as_ref().and_then(|order| order.id.clone()) {
            Some(order_id) => vec![
                next_action(
                    "shopping order get <order-id> --wait",
                    "Review the order after it becomes visible",
                )
                .with_param("order-id", NextActionParam::value(order_id)),
            ],
            None => vec![pending_order_action(checkout_id, show_agent_note)],
        },
        Some("complete_in_progress") => vec![pending_order_action(checkout_id, show_agent_note)],
        Some(status @ ("incomplete" | "ready_for_complete")) => {
            let mut actions = payment_action(checkout, &checkout_id, account_url)
                .into_iter()
                .collect::<Vec<_>>();
            if status == "ready_for_complete" && selected_payment_id(checkout).is_some() {
                actions.push(complete_action(checkout_id, show_agent_note));
            }
            actions
        }
        _ => Vec::new(),
    }
}

/// Follow-up for an order that is being placed but cannot be read yet. It only
/// ever re-checks the checkout session, so retrying cannot place a second order.
pub(super) fn pending_order_action(checkout_id: String, show_agent_note: bool) -> NextAction {
    let mut description =
        "Your order is still being placed and can take a few seconds to appear. Check again shortly; do not place the order again."
            .to_owned();
    if show_agent_note {
        description.push_str(
            " AI assistants: Check a few times at most, then tell the user the order is still pending instead of retrying indefinitely.",
        );
    }
    next_action("shopping checkout get <checkout-id>", description)
        .with_param("checkout-id", NextActionParam::value(checkout_id))
}

fn complete_action(checkout_id: String, show_agent_note: bool) -> NextAction {
    next_action(
        "shopping checkout complete <checkout-id>",
        format!(
            "Place the order. {}",
            agreement_review_description(show_agent_note)
        ),
    )
    .with_param("checkout-id", NextActionParam::value(checkout_id))
}

/// Gets the customer to a selected payment method: add one when none is saved,
/// or pick an eligible saved one. Nothing is suggested once one is selected.
fn payment_action(checkout: &Checkout, checkout_id: &str, account_url: &str) -> Option<NextAction> {
    if selected_payment_id(checkout).is_some() {
        return None;
    }
    if let Some(add_action) = no_saved_payment_method_action(checkout, account_url) {
        return Some(add_action);
    }
    let instrument_id = checkout
        .payment
        .as_ref()?
        .instruments
        .iter()
        .find_map(|instrument| instrument.id.clone())?;
    Some(
        next_action(
            "shopping checkout update <checkout-id> --payment-instrument <instrument-id>",
            "Select a saved payment method before placing the order. Use --show-all-payment-instruments to see every eligible method.",
        )
        .with_param("checkout-id", NextActionParam::value(checkout_id))
        .with_param("instrument-id", NextActionParam::value(instrument_id)),
    )
}

#[derive(Debug, Clone, clap::Args)]
struct Args {
    /// Checkout session ID.
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
                "Review an open checkout session, including its items, available payment methods, required \
                 agreements, and important links. Use the order ID returned after placing an order to review a completed \
                 purchase.",
            )
            .with_system("shopping")
            .with_tier(Tier::Read)
            .with_scopes(SHOPPING_PURCHASE_SCOPES)
            .with_view_id(CHECKOUT_VIEW_ID),
        |ctx, args: Args| async move {
            let client = make_client(&ctx).await?;
            let checkout = crate::shopping::client::get_checkout(&client, &args.id)
                .await
                .map_err(client_err)?;
            reject_response_errors(&checkout.messages)?;
            let env = crate::environments::resolve(&ctx.middleware.env)?;
            let actions = next_actions(
                &checkout,
                &args.id,
                &env.account_url,
                ctx.middleware.output_format != "human",
            );
            let checkout = encode_checkout(&checkout)?;
            let output = if ctx.middleware.output_format == "human" {
                checkout_response(&checkout, args.show_all_payment_instruments)
            } else {
                checkout
            };
            Ok(CommandResult::new(output).with_next_actions(actions))
        },
    )
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use shopping_client::types::{
        OrderConfirmation, Payment, PaymentInstrumentSelectedPaymentInstrument,
    };

    use super::next_actions;
    use crate::shopping::client::Checkout;
    use crate::shopping::human::checkout_response;

    fn instrument(id: &str, selected: bool) -> PaymentInstrumentSelectedPaymentInstrument {
        PaymentInstrumentSelectedPaymentInstrument {
            id: Some(id.to_owned()),
            selected: Some(selected),
            ..Default::default()
        }
    }

    fn checkout(
        status: &str,
        instruments: Vec<PaymentInstrumentSelectedPaymentInstrument>,
    ) -> Checkout {
        Checkout {
            id: Some("checkout-1".to_owned()),
            status: Some(status.to_owned()),
            payment: Some(Payment { instruments }),
            ..Default::default()
        }
    }

    fn assert_no_consent_flag(actions: &[cli_engine::NextAction]) {
        for action in actions {
            assert!(!action.command.contains("--agree"));
            assert!(action.params.values().all(|param| {
                !param
                    .value
                    .as_deref()
                    .unwrap_or_default()
                    .contains("--agree")
            }));
        }
    }

    #[test]
    fn ready_checkout_with_selected_payment_suggests_completion_without_consent() {
        let actions = next_actions(
            &checkout("ready_for_complete", vec![instrument("payment-1", true)]),
            "input-id",
            "https://account.example.test",
            true,
        );

        assert_eq!(actions.len(), 1);
        assert_eq!(
            actions[0].command,
            "gddy shopping checkout complete <checkout-id>"
        );
        assert_eq!(
            actions[0].params["checkout-id"].value.as_deref(),
            Some("checkout-1")
        );
        assert!(
            actions[0]
                .description
                .contains("review every required agreement")
        );
        assert!(actions[0].description.contains("AI assistants:"));
        assert_no_consent_flag(&actions);
    }

    #[test]
    fn human_output_omits_the_agent_note() {
        let actions = next_actions(
            &checkout("ready_for_complete", vec![instrument("payment-1", true)]),
            "input-id",
            "https://account.example.test",
            false,
        );

        assert!(!actions[0].description.contains("AI assistants:"));
    }

    #[test]
    fn ready_checkout_without_selected_payment_suggests_selecting_one() {
        let actions = next_actions(
            &checkout(
                "ready_for_complete",
                vec![
                    instrument("payment-1", false),
                    instrument("payment-2", false),
                ],
            ),
            "input-id",
            "https://account.example.test",
            true,
        );

        assert_eq!(actions.len(), 1);
        assert_eq!(
            actions[0].command,
            "gddy shopping checkout update <checkout-id> --payment-instrument <instrument-id>"
        );
        assert_eq!(
            actions[0].params["instrument-id"].value.as_deref(),
            Some("payment-1")
        );
        assert!(!actions[0].command.contains("complete"));
        assert_no_consent_flag(&actions);
    }

    #[test]
    fn ready_checkout_without_saved_payment_methods_suggests_adding_one() {
        let actions = next_actions(
            &checkout("ready_for_complete", Vec::new()),
            "input-id",
            "https://account.example.test",
            true,
        );

        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].command, "gddy payment-method add");
    }

    #[test]
    fn checkout_in_other_states_gets_no_suggestion() {
        for status in ["requires_escalation", "canceled", "something_new"] {
            let actions = next_actions(
                &checkout(status, vec![instrument("payment-1", true)]),
                "input-id",
                "https://account.example.test",
                true,
            );
            assert!(actions.is_empty(), "{status} must not suggest a next step");
            let without_methods = next_actions(
                &checkout(status, Vec::new()),
                "input-id",
                "https://account.example.test",
                true,
            );
            assert!(
                without_methods.is_empty(),
                "{status} must not suggest adding a payment method"
            );
        }
    }

    #[test]
    fn incomplete_checkout_without_saved_payment_methods_suggests_adding_one() {
        let actions = next_actions(
            &checkout("incomplete", Vec::new()),
            "input-id",
            "https://account.example.test",
            true,
        );

        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].command, "gddy payment-method add");
        assert_no_consent_flag(&actions);
    }

    #[test]
    fn incomplete_checkout_with_unselected_methods_suggests_selecting_one_but_never_completing() {
        let actions = next_actions(
            &checkout("incomplete", vec![instrument("payment-1", false)]),
            "input-id",
            "https://account.example.test",
            true,
        );

        assert_eq!(actions.len(), 1);
        assert!(actions[0].command.contains("checkout update"));
        assert!(!actions[0].command.contains("complete <"));
        assert_no_consent_flag(&actions);
    }

    #[test]
    fn incomplete_checkout_with_a_selected_method_never_suggests_completion() {
        let actions = next_actions(
            &checkout("incomplete", vec![instrument("payment-1", true)]),
            "input-id",
            "https://account.example.test",
            true,
        );

        assert!(actions.is_empty());
    }

    #[test]
    fn order_still_being_placed_suggests_checking_again_never_completing() {
        for status in ["complete_in_progress", "completed"] {
            let actions = next_actions(
                &checkout(status, vec![instrument("payment-1", true)]),
                "input-id",
                "https://account.example.test",
                true,
            );

            assert_eq!(actions.len(), 1, "{status}");
            assert_eq!(
                actions[0].command,
                "gddy shopping checkout get <checkout-id>"
            );
            assert_eq!(
                actions[0].params["checkout-id"].value.as_deref(),
                Some("checkout-1")
            );
            assert!(actions[0].description.contains("few seconds"));
            assert!(
                actions[0]
                    .description
                    .contains("do not place the order again")
            );
            assert!(actions[0].description.contains("AI assistants:"));
            assert_no_consent_flag(&actions);
        }
    }

    #[test]
    fn pending_order_note_for_assistants_is_omitted_for_humans() {
        let action = super::pending_order_action("checkout-1".to_owned(), false);

        assert!(!action.description.contains("AI assistants:"));
    }

    #[test]
    fn completed_checkout_points_at_its_order() {
        let mut completed = checkout("completed", vec![instrument("payment-1", true)]);
        completed.order = Some(OrderConfirmation {
            id: Some("order-1".to_owned()),
            ..Default::default()
        });
        let actions = next_actions(&completed, "input-id", "https://account.example.test", true);

        assert_eq!(actions.len(), 1);
        assert_eq!(
            actions[0].command,
            "gddy shopping order get <order-id> --wait"
        );
        assert_eq!(
            actions[0].params["order-id"].value.as_deref(),
            Some("order-1")
        );
        assert_no_consent_flag(&actions);
    }

    #[test]
    fn falls_back_to_the_input_id_when_the_response_omits_it() {
        let mut ready = checkout("ready_for_complete", vec![instrument("payment-1", true)]);
        ready.id = None;
        let actions = next_actions(&ready, "input-id", "https://account.example.test", true);

        assert_eq!(
            actions[0].params["checkout-id"].value.as_deref(),
            Some("input-id")
        );
    }

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
