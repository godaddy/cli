use cli_engine::{CommandSpec, RuntimeCommandSpec, Tier};

use super::power::{Power, run};
use crate::scopes::{HOSTING_SERVER_EXECUTE as SERVER_EXECUTE, HOSTING_SERVER_READ as SERVER_READ};

#[derive(Debug, Clone, clap::Args)]
struct StopArgs {
    /// Server ID, as shown by `hosting vps list`.
    #[arg(long = "server-id", value_name = "SERVER_ID")]
    server_id: String,

    /// Confirm the action; without it nothing is sent.
    #[arg(long)]
    confirm: bool,

    /// Return at once with the operation ID instead of waiting for it to finish.
    #[arg(long)]
    no_wait: bool,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<StopArgs, _, _, _>(
        CommandSpec::from_args::<StopArgs>("stop", "Stop a server")
            .with_long(
                "Stop a server. The server, and everything running on it, is offline until it is started again. Requires --confirm; without it no request is sent. \
                 Waits for the operation to finish and prints its final state. Use \
                 --no-wait to return at once, then check progress with \
                 `hosting vps operation get`.",
            )
            .with_system("hosting")
            .with_tier(Tier::Destructive)
            .mutates(true)
            .with_scopes(&[SERVER_EXECUTE, SERVER_READ]),
        |ctx, args: StopArgs| async move {
            run(&ctx, Power::Stop, args.server_id, args.confirm, args.no_wait).await
        },
    )
}
