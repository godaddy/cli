use cli_engine::{CommandSpec, RuntimeCommandSpec, Tier};

use super::power::{Power, run};
use crate::scopes::{HOSTING_SERVER_EXECUTE as SERVER_EXECUTE, HOSTING_SERVER_READ as SERVER_READ};

#[derive(Debug, Clone, clap::Args)]
struct RestartArgs {
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
    RuntimeCommandSpec::new_typed_with_context::<RestartArgs, _, _, _>(
        CommandSpec::from_args::<RestartArgs>("restart", "Restart a server")
            .with_long(
                "Restart a running server. The server is offline for a short time. Requires --confirm; without it no request is sent. \
                 Waits for the operation to finish and prints its final state. Use \
                 --no-wait to return at once, then check progress with \
                 `hosting vps operation get`.",
            )
            .with_system("hosting")
            .with_tier(Tier::Destructive)
            .mutates(true)
            .with_scopes(&[SERVER_EXECUTE, SERVER_READ]),
        |ctx, args: RestartArgs| async move {
            run(&ctx, Power::Restart, args.server_id, args.confirm, args.no_wait).await
        },
    )
}
