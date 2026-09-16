use cli_engine::{CommandResult, CommandSpec, RuntimeCommandSpec, Tier};

use crate::shopping::SHOPPING_SCOPES;
use crate::shopping::common::{
    CheckoutInput, client_err, currency_code, make_client, no_saved_payment_method_action,
    reject_multiple_payment_instruments, update_response,
};
use crate::shopping::human::{CHECKOUT_VIEW_ID, checkout_response};

#[derive(Debug, Clone, clap::Args)]
struct Args {
    /// Checkout session ID.
    #[arg(value_name = "CHECKOUT_ID")]
    id: String,

    /// Purchase option ID to include. Repeat for multiple items; append `=QUANTITY` to set a quantity.
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

fn validate_requested_payment(
    checkout: &serde_json::Value,
    requested_payment_instrument: Option<&str>,
) -> cli_engine::Result<()> {
    let Some(requested_payment_instrument) = requested_payment_instrument else {
        return Ok(());
    };
    let is_available = checkout
        .pointer("/payment/instruments")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|instruments| {
            instruments.iter().any(|instrument| {
                instrument.get("id").and_then(serde_json::Value::as_str)
                    == Some(requested_payment_instrument)
            })
        });
    if is_available {
        return Ok(());
    }
    Err(crate::error::GddyError::validation(format!(
        "the requested payment method {requested_payment_instrument:?} is not eligible for this checkout session"
    ))
    .with_fix("Use `shopping checkout get <checkout-id> --show-all-payment-instruments` to find an eligible saved payment method, then retry with --payment-instrument.")
    .into_cli_error())
}

fn validate_selected_payment(
    checkout: &serde_json::Value,
    expected_payment: Option<&str>,
    requested_payment_instrument: Option<&str>,
) -> cli_engine::Result<()> {
    let Some(expected_payment) = requested_payment_instrument.or(expected_payment) else {
        return Ok(());
    };
    let selected_payment = crate::shopping::human::selected_payment_id(checkout);
    if selected_payment == Some(expected_payment) {
        return Ok(());
    }
    let message = if requested_payment_instrument.is_some() {
        format!(
            "the requested payment method {expected_payment:?} is not selected for this checkout session"
        )
    } else {
        format!(
            "the checkout update changed the selected payment method from {expected_payment:?} to {}",
            selected_payment.unwrap_or("none"),
        )
    };
    Err(crate::error::GddyError::validation(message)
        .with_fix("Review the updated checkout session, then select an eligible saved payment method with --payment-instrument before completing the purchase.")
        .into_cli_error())
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<Args, _, _, _>(
        CommandSpec::from_args::<Args>("update", "Optionally update a checkout session")
            .with_long(
                "Update an open checkout session before placing an order. The CLI retrieves its current \
                 state and preserves unchanged writable fields while applying your item, buyer, currency, \
                 or payment-method changes. The response shows the currently selected and eligible saved payment \
                 methods (the first five by default; use --show-all-payment-instruments for all). If a requested \
                 payment method is not selected, the command reports it as ineligible and shows how to choose another.",
            )
            .with_system("shopping")
            .with_tier(Tier::Mutate)
            .mutates(true)
            .handles_dry_run(true)
            .with_scopes(SHOPPING_SCOPES)
            .auth_optional()
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
            let client = make_client(&ctx).await?;
            let current_checkout = client
                .get_checkout(&args.id)
                .await
                .map_err(client_err)?;
            let previously_selected_payment = crate::shopping::human::selected_payment_id(&current_checkout);
            let requested_payment_instrument = input.payment_instrument.clone();
            validate_requested_payment(&current_checkout, requested_payment_instrument.as_deref())?;
            let body = input.update_body(&current_checkout)?;
            reject_multiple_payment_instruments(&body)?;
            if ctx.dry_run() {
                return Ok(CommandResult::new(serde_json::json!({
                    "action": "dry-run: would update checkout",
                    "id": args.id,
                    "body": body,
                }))
                .with_dry_run());
            }
            update_response(
                client
                    .update_checkout(&args.id, body, &uuid::Uuid::new_v4().to_string())
                    .await
                    .map_err(client_err)?,
            )?;
            let checkout = client.get_checkout(&args.id).await.map_err(client_err)?;
            validate_selected_payment(
                &checkout,
                previously_selected_payment,
                requested_payment_instrument.as_deref(),
            )?;
            let env = crate::environments::resolve(&ctx.middleware.env)?;
            let actions = no_saved_payment_method_action(&checkout, &env.account_url)
            .into_iter()
            .collect::<Vec<_>>();
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

    use super::{validate_requested_payment, validate_selected_payment};

    #[test]
    fn rejects_a_payment_method_not_offered_by_the_checkout() {
        let error = validate_requested_payment(
            &json!({"payment": {"instruments": [{"id": "payment-1", "selected": true}]}}),
            Some("payment-2"),
        )
        .expect_err("unavailable payment method must be rejected");
        assert!(
            error
                .to_string()
                .contains("requested payment method \"payment-2\" is not eligible")
        );
    }

    #[test]
    fn rejects_a_silently_substituted_payment_method() {
        let error = validate_selected_payment(
            &json!({"payment": {"instruments": [{"id": "payment-1", "selected": true}]}}),
            Some("payment-2"),
            Some("payment-2"),
        )
        .expect_err("a substituted payment method must be reported");
        assert!(
            error
                .to_string()
                .contains("requested payment method \"payment-2\" is not selected")
        );
    }

    #[test]
    fn accepts_the_expected_selected_payment_method() {
        assert!(
            validate_selected_payment(
                &json!({"payment": {"instruments": [{"id": "payment-2", "selected": true}]}}),
                Some("payment-2"),
                None,
            )
            .is_ok()
        );
    }
}
