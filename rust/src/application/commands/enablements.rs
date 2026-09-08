//! `gddy platform app enablements` — list applications enabled for a store.

use cli_engine::{CommandResult, CommandSpec, NextActionParam, RuntimeCommandSpec, Tier};

use super::schemas::EnabledApplication;
use crate::next_action::{next_action, required_value};
use crate::scopes::APP_REGISTRY_READ;

#[derive(Debug, Clone, clap::Args)]
struct EnablementsArgs {
    /// Store ID whose enabled applications should be listed.
    #[arg(long = "store-id", value_name = "STORE_ID")]
    store_id: String,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<EnablementsArgs, _, _, _>(
        CommandSpec::from_args::<EnablementsArgs>(
            "enablements",
            "List applications enabled on a store",
        )
        .with_long(
            "List all GoDaddy developer-platform applications enabled for a \
            store by its store ID.",
        )
        .with_system("applications")
        .with_tier(Tier::Read)
        .with_scopes(&[APP_REGISTRY_READ])
        .with_default_fields("name,label,status")
        .with_output_schema::<EnabledApplication>(),
        |ctx, args: EnablementsArgs| async move {
            let store_id = args.store_id;
            let client = super::make_client(&ctx).await?;
            let applications = client
                .list_enabled_applications(&store_id)
                .await
                .map_err(super::client_err)?;

            Ok(CommandResult::new(applications).with_next_actions(vec![
                next_action(
                    "platform app info --name <name>",
                    "Inspect one of the enabled applications",
                )
                .with_param("name", NextActionParam::required()),
                next_action(
                    "platform app disable <name> --store-id <store-id>",
                    "Disable an application on this store",
                )
                .with_param("name", NextActionParam::required())
                .with_param("store-id", required_value(&store_id)),
            ]))
        },
    )
}
