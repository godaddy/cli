use cli_engine::{CommandResult, CommandSpec, RuntimeCommandSpec, Tier};
use shopping_client::types::Checkout;

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
    checkout: &Checkout,
    requested_payment_instrument: Option<&str>,
) -> cli_engine::Result<()> {
    let Some(requested_payment_instrument) = requested_payment_instrument else {
        return Ok(());
    };
    let is_available = checkout.payment.as_ref().is_some_and(|payment| {
        payment
            .instruments
            .iter()
            .any(|instrument| instrument.id.as_deref() == Some(requested_payment_instrument))
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
    checkout: &Checkout,
    expected_payment: Option<&str>,
    requested_payment_instrument: Option<&str>,
) -> cli_engine::Result<()> {
    let Some(expected_payment) = requested_payment_instrument.or(expected_payment) else {
        return Ok(());
    };
    let selected_payment = crate::shopping::common::selected_payment_id(checkout);
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
            let current_checkout = crate::shopping::client::get_checkout(&client, &args.id)
                .await
                .map_err(client_err)?;
            let previously_selected_payment =
                crate::shopping::common::selected_payment_id(&current_checkout).map(str::to_owned);
            let requested_payment_instrument = input.payment_instrument.clone();
            validate_requested_payment(&current_checkout, requested_payment_instrument.as_deref())?;
            let body = input.update_body(&current_checkout)?;
            let instruments = body
                .0
                .payment
                .as_ref()
                .map_or(&[][..], |payment| payment.0.instruments.as_slice());
            reject_multiple_payment_instruments(instruments)?;
            if ctx.dry_run() {
                return Ok(CommandResult::new(serde_json::json!({
                    "action": "dry-run: would update checkout",
                    "id": args.id,
                    "body": serde_json::to_value(&body).map_err(|error| {
                        crate::error::GddyError::unexpected(format!(
                            "failed to encode checkout update request: {error}"
                        ))
                        .into_cli_error()
                    })?,
                }))
                .with_dry_run());
            }
            update_response(
                crate::shopping::client::update_checkout(
                    &client,
                    &args.id,
                    body,
                    &uuid::Uuid::new_v4().to_string(),
                )
                .await
                .map_err(client_err)?,
            )?;
            let checkout = crate::shopping::client::get_checkout(&client, &args.id)
                .await
                .map_err(client_err)?;
            validate_selected_payment(
                &checkout,
                previously_selected_payment.as_deref(),
                requested_payment_instrument.as_deref(),
            )?;
            let env = crate::environments::resolve(&ctx.middleware.env)?;
            let actions = no_saved_payment_method_action(&checkout, &env.account_url)
            .into_iter()
            .collect::<Vec<_>>();
            let checkout = serde_json::to_value(&checkout).map_err(|error| {
                crate::error::GddyError::unexpected(format!(
                    "failed to encode checkout response: {error}"
                ))
                .into_cli_error()
            })?;
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
    use shopping_client::types::{Checkout, Payment, PaymentInstrumentSelectedPaymentInstrument};

    use super::{validate_requested_payment, validate_selected_payment};

    fn checkout_with_instrument(id: &str) -> Checkout {
        Checkout {
            payment: Some(Payment {
                instruments: vec![PaymentInstrumentSelectedPaymentInstrument {
                    id: Some(id.to_owned()),
                    selected: Some(true),
                    billing_address: Default::default(),
                }],
            }),
            ..Default::default()
        }
    }

    #[test]
    fn rejects_a_payment_method_not_offered_by_the_checkout() {
        let error =
            validate_requested_payment(&checkout_with_instrument("payment-1"), Some("payment-2"))
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
            &checkout_with_instrument("payment-1"),
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
                &checkout_with_instrument("payment-2"),
                Some("payment-2"),
                None
            )
            .is_ok()
        );
    }
}
