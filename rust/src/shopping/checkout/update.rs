use cli_engine::{CommandResult, CommandSpec, RuntimeCommandSpec, Tier};

use crate::shopping::SHOPPING_SCOPES;
use crate::shopping::common::{client_err, has_conflicting_checkout_id, make_client, read_json};

#[derive(Debug, Clone, clap::Args)]
struct Args {
    /// Checkout session ID.
    #[arg(value_name = "CHECKOUT_ID")]
    id: String,

    /// Full checkout replacement as raw JSON.
    #[arg(long, value_name = "JSON", required_unless_present = "file")]
    body: Option<String>,

    /// Path to a JSON checkout replacement. Takes precedence over --body.
    #[arg(long, value_name = "PATH")]
    file: Option<String>,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<Args, _, _, _>(
        CommandSpec::from_args::<Args>("update", "Fully replace a Shopping checkout")
            .with_long(
                "Replace checkout fields with a UCP JSON object. The request must include \
                 line_items; use an empty array only to deliberately clear the cart.",
            )
            .with_system("shopping")
            .with_tier(Tier::Mutate)
            .mutates(true)
            .handles_dry_run(true)
            .with_scopes(SHOPPING_SCOPES)
            .auth_optional(),
        |ctx, args: Args| async move {
            let body = read_json(args.body.as_deref(), args.file.as_deref(), "object")?;
            if has_conflicting_checkout_id(&body, &args.id) {
                return Err(crate::error::GddyError::validation(
                    "checkout ID in request body conflicts with CHECKOUT_ID",
                )
                .into_cli_error());
            }
            if ctx.dry_run() {
                return Ok(CommandResult::new(serde_json::json!({
                    "action": "dry-run: would update checkout",
                    "id": args.id,
                    "body": body,
                })));
            }
            let client = make_client(&ctx).await?;
            Ok(CommandResult::new(
                client
                    .update_checkout(&args.id, body)
                    .await
                    .map_err(client_err)?,
            ))
        },
    )
}
