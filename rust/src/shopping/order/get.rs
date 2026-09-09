use cli_engine::{CommandResult, CommandSpec, RuntimeCommandSpec, Tier};

use crate::output_schema::output_schema;
use crate::shopping::SHOPPING_SCOPES;
use crate::shopping::common::{client_err, make_client, wait_duration, wait_for_order};

output_schema!(OrderOutput {
    "ucp": "object";
    "id": "string";
    "checkout_id": "string";
    "line_items": "[]object";
    "totals": "[]object";
});

#[derive(Debug, Clone, clap::Args)]
struct Args {
    /// Completed order ID returned by checkout completion.
    #[arg(value_name = "ORDER_ID")]
    id: String,

    /// Poll until the new order becomes available.
    #[arg(long)]
    wait: bool,

    /// Maximum seconds to wait for order visibility (1-60, default 15).
    #[arg(long, value_name = "SECONDS", requires = "wait")]
    wait_timeout: Option<u8>,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<Args, _, _, _>(
        CommandSpec::from_args::<Args>("get", "Get a completed Shopping order")
            .with_long(
                "Read a completed order. New orders usually become available within 3-10 seconds; \
                 use --wait to poll for up to 15 seconds by default.",
            )
            .with_system("shopping")
            .with_tier(Tier::Read)
            .with_scopes(SHOPPING_SCOPES)
            .with_output_schema::<OrderOutput>(),
        |ctx, args: Args| async move {
            let client = make_client(&ctx).await?;
            if args.wait {
                let (order, _) =
                    wait_for_order(&client, &args.id, wait_duration(args.wait_timeout)?).await?;
                Ok(CommandResult::new(order))
            } else {
                Ok(CommandResult::new(
                    client.get_order(&args.id).await.map_err(client_err)?,
                ))
            }
        },
    )
}
