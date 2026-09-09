use cli_engine::{CommandResult, CommandSpec, RuntimeCommandSpec, Tier};
use serde_json::Value;

use crate::next_action::{next_action, required_value};
use crate::output_schema::output_schema;
use crate::shopping::common::{client_err, make_client, read_json};
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
    /// Checkout-create request as raw JSON.
    #[arg(long, value_name = "JSON", required_unless_present = "file")]
    body: Option<String>,

    /// Path to a JSON checkout-create request. Takes precedence over --body.
    #[arg(long, value_name = "PATH")]
    file: Option<String>,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<Args, _, _, _>(
        CommandSpec::from_args::<Args>("create", "Create a Shopping checkout session")
            .with_long(
                "Create a checkout from a Shopping API request object. Supply it with --body or \
                 --file; `gddy guide shopping` documents the required line_items format. This \
                 creates a checkout but does not place an order; use `checkout complete` only after \
                 reviewing the checkout.",
            )
            .with_system("shopping")
            .with_tier(Tier::Mutate)
            .mutates(true)
            .handles_dry_run(true)
            .with_scopes(SHOPPING_SCOPES)
            .auth_optional()
            .with_output_schema::<CheckoutOutput>(),
        |ctx, args: Args| async move {
            let body = read_json(args.body.as_deref(), args.file.as_deref(), "object")?;
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
            let mut result = CommandResult::new(checkout);
            if ready_for_complete {
                result = result.with_next_actions(vec![
                    next_action(
                        command_for_env(
                            &ctx.middleware.env,
                            format!("checkout complete {checkout_id} --file complete-checkout.json"),
                        ),
                        "Complete this checkout with a selected saved payment instrument",
                    )
                    .with_param("checkout_id", required_value(checkout_id)),
                ]);
            }
            Ok(result)
        },
    )
}
