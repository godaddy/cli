use cli_engine::{CommandResult, CommandSpec, NextActionParam, RuntimeCommandSpec, Tier};

use crate::hosting::common::{HostingServer, client_err, make_client};
use crate::next_action::next_action;
use crate::scopes::HOSTING_SERVER_READ as SERVER_READ;

#[derive(Debug, Clone, clap::Args)]
pub(super) struct ServerIdArgs {
    /// Server ID, as shown by `hosting vps list`.
    #[arg(long = "server-id", value_name = "SERVER_ID")]
    pub server_id: String,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<ServerIdArgs, _, _, _>(
        CommandSpec::from_args::<ServerIdArgs>("get", "Show one server in detail")
            .with_long(
                "Show one server: its name, status, IP address, operating system and \
                 plan. `status` is the platform's own value, for example ACTIVE \
                 (running) or STOPPED.",
            )
            .with_system("hosting")
            .with_tier(Tier::Read)
            .with_scopes(&[SERVER_READ])
            .with_output_schema::<HostingServer>(),
        |ctx, args: ServerIdArgs| async move {
            let client = make_client(&ctx, &[SERVER_READ]).await?;
            let data = client
                .get_server(&args.server_id)
                .await
                .map_err(client_err)?;
            Ok(CommandResult::new(data).with_next_actions(vec![
                next_action(
                    "hosting vps restart --server-id <server-id> --confirm",
                    "Restart this server",
                )
                .with_param("server-id", NextActionParam::value(args.server_id)),
            ]))
        },
    )
}
