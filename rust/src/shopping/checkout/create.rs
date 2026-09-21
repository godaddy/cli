use cli_engine::{
    CommandResult, CommandSpec, NextAction, NextActionParam, RuntimeCommandSpec, Tier,
};

use crate::next_action::next_action;
use crate::output_schema::output_schema;
use crate::shopping::AGENT_AGREEMENT_CONFIRMATION_INSTRUCTIONS;
use crate::shopping::SHOPPING_SCOPES;
use crate::shopping::common::{
    CheckoutInput, client_err, currency_code, make_client, no_saved_payment_method_action,
    reject_multiple_payment_instruments, update_response,
};
use crate::shopping::human::{CHECKOUT_VIEW_ID, checkout_response, empty_acknowledgement_response};

output_schema!(CheckoutOutput {
    "ucp": "object";
    "id": "string";
    "status": "string";
    "line_items": "[]object";
    "totals": "[]object";
    "payment": "object", optional;
    "messages": "[]object";
    "action": "string", optional;
    "body": "object", optional;
});

#[derive(Debug, Clone, clap::Args)]
struct Args {
    /// Purchase option ID to add to the checkout session. Repeat for multiple items; append `=QUANTITY` to set a quantity.
    #[arg(long, value_name = "PURCHASE_OPTION_ID[=QUANTITY]")]
    item: Vec<String>,

    /// Preferred ISO 4217 currency for checkout prices (for example, USD or GBP).
    #[arg(long, value_name = "CODE", value_parser = currency_code)]
    currency: Option<String>,

    /// Buyer's first name.
    #[arg(long, value_name = "NAME")]
    buyer_first_name: Option<String>,

    /// Buyer's last name.
    #[arg(long, value_name = "NAME")]
    buyer_last_name: Option<String>,

    /// Buyer's email address.
    #[arg(long, value_name = "EMAIL")]
    buyer_email: Option<String>,

    /// Buyer's phone number.
    #[arg(long, value_name = "PHONE")]
    buyer_phone: Option<String>,

    /// Select one saved payment instrument by ID.
    #[arg(long, value_name = "INSTRUMENT_ID")]
    payment_instrument: Option<String>,

    /// Show every available saved payment instrument instead of the first five.
    #[arg(long)]
    show_all_payment_instruments: bool,
}

fn agreement_review_action(checkout_id: String) -> NextAction {
    next_action(
        "shopping checkout get <checkout-id>",
        format!(
            "Before completing checkout, review every required agreement and important link. The --agree flag on checkout completion acknowledges and accepts all required agreements; use it only after that review. {AGENT_AGREEMENT_CONFIRMATION_INSTRUCTIONS}"
        ),
    )
    .with_param("checkout-id", NextActionParam::value(checkout_id))
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<Args, _, _, _>(
        CommandSpec::from_args::<Args>("create", "Create a checkout session")
            .with_long(
                "Add one or more purchase options to a checkout session. Repeat --item for multiple \
                 purchase options and append =QUANTITY when needed. Creating a checkout session does not \
                 place an order. Its response shows the currently selected and eligible saved payment methods \
                 (the first five by default; use --show-all-payment-instruments for all), required agreements, \
                 and important links to review before placing an order.",
            )
            .with_system("shopping")
            .with_tier(Tier::Mutate)
            .mutates(true)
            .handles_dry_run(true)
            .with_scopes(SHOPPING_SCOPES)
            .auth_optional()
            .with_output_schema::<CheckoutOutput>()
            .with_view_id(CHECKOUT_VIEW_ID),
        |ctx, args: Args| async move {
            let input = CheckoutInput {
                items: args.item,
                currency: args.currency,
                buyer_first_name: args.buyer_first_name,
                buyer_last_name: args.buyer_last_name,
                buyer_email: args.buyer_email,
                buyer_phone: args.buyer_phone,
                payment_instrument: args.payment_instrument,
            };
            let body = input.create_body()?;
            let instruments = body
                .0
                .payment
                .as_ref()
                .map_or(&[][..], |payment| payment.0.instruments.as_slice());
            reject_multiple_payment_instruments(instruments)?;
            if ctx.dry_run() {
                return Ok(CommandResult::new(serde_json::json!({
                    "action": "dry-run: would create checkout",
                    "body": serde_json::to_value(&body).map_err(|error| {
                        crate::error::GddyError::unexpected(format!(
                            "failed to encode checkout create request: {error}"
                        ))
                        .into_cli_error()
                    })?,
                }))
                .with_dry_run());
            }
            let client = make_client(&ctx).await?;
            let checkout = crate::shopping::client::create_checkout(
                &client,
                body,
                &uuid::Uuid::new_v4().to_string(),
            )
            .await
            .map_err(client_err)?
            .map(update_response)
            .transpose()?;
            let is_acknowledgement = checkout.is_none();
            let ready_for_complete = checkout
                .as_ref()
                .and_then(|checkout| checkout.status.as_deref())
                == Some("ready_for_complete");
            let checkout_id = checkout
                .as_ref()
                .and_then(|checkout| checkout.id.clone())
                .unwrap_or_default();
            let env = crate::environments::resolve(&ctx.middleware.env)?;
            let mut actions = checkout
                .as_ref()
                .and_then(|checkout| no_saved_payment_method_action(checkout, &env.account_url))
                .into_iter()
                .collect::<Vec<_>>();
            if ready_for_complete {
                actions.push(agreement_review_action(checkout_id));
            }
            let checkout = serde_json::to_value(&checkout).map_err(|error| {
                crate::error::GddyError::unexpected(format!(
                    "failed to encode checkout response: {error}"
                ))
                .into_cli_error()
            })?;
            let output = if ctx.middleware.output_format == "human" {
                if is_acknowledgement {
                    empty_acknowledgement_response(
                        "The Shopping API accepted the request but hasn't returned checkout details yet.",
                    )
                } else {
                    checkout_response(&checkout, args.show_all_payment_instruments)
                }
            } else {
                checkout
            };
            Ok(CommandResult::new(output).with_next_actions(actions))
        },
    )
}

#[cfg(test)]
mod tests {
    use super::agreement_review_action;

    #[test]
    fn agreement_review_action_never_suggests_checkout_completion() {
        let action = agreement_review_action("checkout-1".to_owned());

        assert_eq!(action.command, "gddy shopping checkout get <checkout-id>");
        assert_eq!(
            action.params["checkout-id"].value.as_deref(),
            Some("checkout-1")
        );
        assert!(
            action
                .description
                .contains("review every required agreement")
        );
        assert!(action.description.contains(
            "--agree flag on checkout completion acknowledges and accepts all required agreements"
        ));
        assert!(action.description.contains("AI assistants:"));
        assert!(
            action
                .description
                .contains("Do not infer agreement from a request to purchase")
        );
        assert!(!action.command.contains("complete"));
        assert!(!action.command.contains("--agree"));
    }
}
