use cli_engine::{CommandResult, CommandSpec, RuntimeCommandSpec, Tier};

use crate::output_schema::output_schema;
use crate::shopping::SHOPPING_SCOPES;
use crate::shopping::common::{client_err, make_client, read_json};

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
                "Create a checkout from a UCP JSON object. This mutates the remote Shopping \
                 service but does not purchase; use `checkout complete` only after review.",
            )
            .with_system("shopping")
            .with_tier(Tier::Mutate)
            .mutates(true)
            .handles_dry_run(true)
            .with_scopes(SHOPPING_SCOPES)
            .auth_optional()
            .with_output_schema::<CheckoutOutput>()
            .with_default_fields("id,status,line_items,totals,messages,action,body"),
        |ctx, args: Args| async move {
            let body = read_json(args.body.as_deref(), args.file.as_deref(), "object")?;
            if ctx.dry_run() {
                return Ok(CommandResult::new(serde_json::json!({
                    "action": "dry-run: would create checkout",
                    "body": body,
                })));
            }
            let client = make_client(&ctx).await?;
            Ok(CommandResult::new(
                client.create_checkout(body).await.map_err(client_err)?,
            ))
        },
    )
}
