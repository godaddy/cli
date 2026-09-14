use cli_engine::{CommandResult, CommandSpec, RuntimeCommandSpec, Tier};

use crate::shopping::SHOPPING_SCOPES;
use crate::shopping::common::{
    CheckoutInput, client_err, currency_code, make_client, no_saved_payment_method_action,
    reject_multiple_payment_instruments,
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

    /// Deliberately replace the checkout session with no items.
    #[arg(long)]
    clear_items: bool,

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
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<Args, _, _, _>(
        CommandSpec::from_args::<Args>("update", "Optionally update a checkout session")
            .with_long(
                "Optionally replace an open checkout session before placing an order. Use --item and \
                 buyer or payment-method flags for common changes, or --clear-items to deliberately \
                 empty the checkout session.",
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
                clear_items: args.clear_items,
                currency: args.currency,
                buyer_first_name: args.buyer_first_name,
                buyer_last_name: args.buyer_last_name,
                buyer_email: args.buyer_email,
                buyer_phone: args.buyer_phone,
                payment_instrument: args.payment_instrument,
            };
            let body = input.update_body()?;
            reject_multiple_payment_instruments(&body)?;
            if ctx.dry_run() {
                return Ok(CommandResult::new(serde_json::json!({
                    "action": "dry-run: would update checkout",
                    "id": args.id,
                    "body": body,
                }))
                .with_dry_run());
            }
            let client = make_client(&ctx).await?;
            let checkout = client
                .update_checkout(&args.id, body)
                .await
                .map_err(client_err)?;
            let env = crate::environments::resolve(&ctx.middleware.env)?;
            let actions = no_saved_payment_method_action(&checkout, &env.account_url)
            .into_iter()
            .collect::<Vec<_>>();
            let output = if ctx.middleware.output_format == "human" {
                checkout_response(&checkout, false)
            } else {
                checkout
            };
            Ok(CommandResult::new(output).with_next_actions(actions))
        },
    )
}
