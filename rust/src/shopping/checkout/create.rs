use cli_engine::{CommandResult, CommandSpec, RuntimeCommandSpec, Tier};
use serde_json::Value;

use crate::next_action::{next_action, required_value};
use crate::output_schema::output_schema;
use crate::shopping::checkout::get::{HUMAN_VIEW_ID, human_response};
use crate::shopping::common::{
    CheckoutInput, client_err, currency_code, make_client, no_saved_payment_method_action,
    read_json, reject_mixed_checkout_input, reject_multiple_payment_instruments,
};
use crate::shopping::{SHOPPING_SCOPES, command_for_env};

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
    /// Variant ID to add to the checkout. Repeat for multiple items; append `=QUANTITY` to set a quantity.
    #[arg(long, value_name = "VARIANT_ID[=QUANTITY]")]
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

    /// Checkout-create request as raw JSON for advanced Shopping API fields.
    #[arg(long, value_name = "JSON")]
    body: Option<String>,

    /// Path to a JSON checkout-create request. Takes precedence over --body.
    #[arg(long, value_name = "PATH")]
    file: Option<String>,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<Args, _, _, _>(
        CommandSpec::from_args::<Args>("create", "Create a Shopping checkout session")
            .with_long(
                "Create a checkout with --item and optional buyer/payment flags. Repeat --item for \
                 multiple variants and append =QUANTITY when needed. Use --body or --file for \
                 advanced Shopping API fields such as item input, fulfillment, or billing addresses. \
                 This creates a checkout but does not place an order; complete it only after review.",
            )
            .with_system("shopping")
            .with_tier(Tier::Mutate)
            .mutates(true)
            .handles_dry_run(true)
            .with_scopes(SHOPPING_SCOPES)
            .auth_optional()
            .with_output_schema::<CheckoutOutput>()
            .with_view_id(HUMAN_VIEW_ID),
        |ctx, args: Args| async move {
            let input = CheckoutInput {
                items: args.item,
                currency: args.currency,
                buyer_first_name: args.buyer_first_name,
                buyer_last_name: args.buyer_last_name,
                buyer_email: args.buyer_email,
                buyer_phone: args.buyer_phone,
                payment_instrument: args.payment_instrument,
                ..CheckoutInput::default()
            };
            reject_mixed_checkout_input(args.body.as_deref(), args.file.as_deref(), input.is_present())?;
            let body = if args.body.is_some() || args.file.is_some() {
                read_json(args.body.as_deref(), args.file.as_deref(), "object")?
            } else {
                input.create_body()?
            };
            reject_multiple_payment_instruments(&body)?;
            if ctx.dry_run() {
                return Ok(CommandResult::new(serde_json::json!({
                    "action": "dry-run: would create checkout",
                    "body": body,
                })));
            }
            let client = make_client(&ctx).await?;
            let checkout = client.create_checkout(body).await.map_err(client_err)?;
            let ready_for_complete =
                checkout.get("status").and_then(Value::as_str) == Some("ready_for_complete");
            let checkout_id = checkout
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            let env = crate::environments::resolve(&ctx.middleware.env)?;
            let mut actions = no_saved_payment_method_action(
                &checkout,
                &ctx.middleware.env,
                &env.account_url,
            )
            .into_iter()
            .collect::<Vec<_>>();
            if ready_for_complete {
                let payment_instrument = checkout
                    .pointer("/payment/instruments")
                    .and_then(Value::as_array)
                    .and_then(|instruments| {
                        instruments.iter().find(|instrument| {
                            instrument.get("selected").and_then(Value::as_bool) == Some(true)
                        })
                    })
                    .and_then(|instrument| instrument.get("id"))
                    .and_then(Value::as_str)
                    .unwrap_or("<instrument-id>");
                actions.push(
                    next_action(
                        command_for_env(
                            &ctx.middleware.env,
                            format!("checkout complete {checkout_id} --payment-instrument {payment_instrument}"),
                        ),
                        "Complete this checkout with a selected saved payment instrument",
                    )
                    .with_param("checkout_id", required_value(checkout_id)),
                );
            }
            let output = if ctx.middleware.output_format == "human" {
                human_response(&checkout, &actions)
            } else {
                checkout
            };
            Ok(CommandResult::new(output).with_next_actions(actions))
        },
    )
}
