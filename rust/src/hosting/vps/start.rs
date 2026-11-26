use cli_engine::{CommandSpec, RuntimeCommandSpec, Tier};

use super::power::{Power, run};
use crate::scopes::{HOSTING_SERVER_EXECUTE as SERVER_EXECUTE, HOSTING_SERVER_READ as SERVER_READ};

#[derive(Debug, Clone, clap::Args)]
struct StartArgs {
    /// Server ID, as shown by `hosting vps list`.
    #[arg(long = "server-id", value_name = "SERVER_ID")]
    server_id: String,

    /// Return at once with the operation ID instead of waiting for it to finish.
    #[arg(long)]
    no_wait: bool,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<StartArgs, _, _, _>(
        CommandSpec::from_args::<StartArgs>("start", "Start a stopped server")
            .with_long(
                "Start a stopped server. Waits for the operation to finish and prints its \
                 final state. Use --no-wait to return at once, then check progress with \
                 `hosting vps operation get`.",
            )
            .with_system("hosting")
            .with_tier(Tier::Mutate)
            .mutates(true)
            .with_scopes(&[SERVER_EXECUTE, SERVER_READ]),
        |ctx, args: StartArgs| async move {
            run(&ctx, Power::Start, args.server_id, true, args.no_wait).await
        },
    )
}
