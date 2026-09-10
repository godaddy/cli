use cli_engine::{CommandResult, CommandSpec, RuntimeCommandSpec, Tier};

use crate::shopping::SHOPPING_SCOPES;
use crate::shopping::checkout::get::HUMAN_VIEW_ID;
use crate::shopping::common::{
    CheckoutInput, client_err, currency_code, has_conflicting_checkout_id, make_client,
    no_saved_payment_method_action, read_json, reject_mixed_checkout_input,
    reject_multiple_payment_instruments,
};

#[derive(Debug, Clone, clap::Args)]
struct Args {
    /// Checkout session ID.
    #[arg(value_name = "CHECKOUT_ID")]
    id: String,

    /// Variant ID to include. Repeat for multiple items; append `=QUANTITY` to set a quantity.
    #[arg(long, value_name = "VARIANT_ID[=QUANTITY]")]
    item: Vec<String>,

    /// Deliberately replace the cart with no items.
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

    /// Full checkout replacement as raw JSON for advanced Shopping API fields.
    #[arg(long, value_name = "JSON")]
    body: Option<String>,

    /// Path to a JSON checkout replacement. Takes precedence over --body.
    #[arg(long, value_name = "PATH")]
    file: Option<String>,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<Args, _, _, _>(
        CommandSpec::from_args::<Args>("update", "Optionally update a Shopping checkout")
            .with_long(
                "Optionally replace an open checkout before completion. Use --item and optional \
                 buyer/payment flags for common changes, or --clear-items to deliberately empty the \
                 cart. Updates replace checkout state; use --body or --file for advanced fields.",
            )
            .with_system("shopping")
            .with_tier(Tier::Mutate)
            .mutates(true)
            .handles_dry_run(true)
            .with_scopes(SHOPPING_SCOPES)
            .auth_optional()
            .with_view_id(HUMAN_VIEW_ID),
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
            reject_mixed_checkout_input(args.body.as_deref(), args.file.as_deref(), input.is_present())?;
            let body = if args.body.is_some() || args.file.is_some() {
                read_json(args.body.as_deref(), args.file.as_deref(), "object")?
            } else {
                input.update_body()?
            };
            if has_conflicting_checkout_id(&body, &args.id) {
                return Err(crate::error::GddyError::validation(
                    "checkout ID in request body conflicts with CHECKOUT_ID",
                )
                .into_cli_error());
            }
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
            let actions = no_saved_payment_method_action(
                &checkout,
                &ctx.middleware.env,
                &env.account_url,
            )
            .into_iter()
            .collect::<Vec<_>>();
            let output = if ctx.middleware.output_format == "human" {
                crate::shopping::checkout::get::human_response(&checkout, &actions, false)
            } else {
                checkout
            };
            Ok(CommandResult::new(output).with_next_actions(actions))
        },
    )
}
