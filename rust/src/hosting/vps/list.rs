use cli_engine::{CommandResult, CommandSpec, NextActionParam, RuntimeCommandSpec, Tier};
use serde_json::{Value, json};

use crate::hosting::common::{HostingServerSummary, client_err, make_client};
use crate::next_action::next_action;
use crate::scopes::HOSTING_SERVER_READ as SERVER_READ;

#[derive(Debug, Clone, clap::Args)]
struct VpsListArgs {
    /// Maximum number of servers to return. Omit to return all.
    #[arg(long, value_name = "N", value_parser = clap::value_parser!(u32).range(1..))]
    limit: Option<u32>,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<VpsListArgs, _, _, _>(
        CommandSpec::from_args::<VpsListArgs>("list", "List your virtual private servers")
            .with_long(
                "List every virtual private server in your account, across all data centers, \
                 in one call. Each row shows the server's name, ID, status, operating system, \
                 control panel, creation date and plan expiry date. Use --limit to cap how \
                 many are shown.",
            )
            .with_system("hosting")
            .with_tier(Tier::Read)
            .with_scopes(&[SERVER_READ])
            .with_default_fields("name,id,status,os,controlPanel,createdAt,expiresAt")
            .with_output_schema::<HostingServerSummary>(),
        |ctx, args: VpsListArgs| async move {
            let client = make_client(&ctx, &[SERVER_READ]).await?;
            let response = client.list_servers().await.map_err(client_err)?;
            let mut all_items: Vec<Value> = response
                .get("items")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            if let Some(cap) = args.limit {
                all_items.truncate(cap as usize);
            }

            Ok(CommandResult::new(json!(all_items)).with_next_actions(vec![
                next_action(
                    "hosting vps get --server-id <server-id>",
                    "Show one server in detail",
                )
                .with_param("server-id", NextActionParam::required()),
            ]))
        },
    )
}
