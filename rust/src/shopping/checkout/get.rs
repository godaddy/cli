use cli_engine::{CommandResult, CommandSpec, RuntimeCommandSpec, Tier};

use crate::shopping::SHOPPING_SCOPES;
use crate::shopping::common::{client_err, make_client};

#[derive(Debug, Clone, clap::Args)]
struct Args {
    /// Checkout session ID.
    #[arg(value_name = "CHECKOUT_ID")]
    id: String,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<Args, _, _, _>(
        CommandSpec::from_args::<Args>("get", "Get an open Shopping checkout session")
            .with_long(
                "Get an open checkout session. Do not use after completion: the current Order \
                 Management service reconstructs it from an open basket. Use `shopping order get` \
                 with the order ID returned by completion instead.",
            )
            .with_system("shopping")
            .with_tier(Tier::Read)
            .with_scopes(SHOPPING_SCOPES),
        |ctx, args: Args| async move {
            let client = make_client(&ctx).await?;
            Ok(CommandResult::new(
                client.get_checkout(&args.id).await.map_err(client_err)?,
            ))
        },
    )
}
