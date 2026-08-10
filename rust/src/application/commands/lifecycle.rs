//! `gddy platform app enable`/`disable`/`archive` — application state transitions.

use cli_engine::{CommandResult, CommandSpec, RuntimeCommandSpec, Tier};
use serde_json::json;

use super::schemas::{ApplicationArchive, ApplicationRef};
use super::validate::ValidateArgs;
use crate::next_action::{next_action, required_value};
use crate::scopes::{APP_REGISTRY_READ, APP_REGISTRY_WRITE};

#[derive(Debug, Clone, clap::Args)]
struct EnableDisableArgs {
    /// Application name.
    #[arg(value_name = "NAME")]
    name: String,

    /// Store ID to enable/disable the application on.
    #[arg(long = "store-id", value_name = "STORE_ID")]
    store_id: String,
}

pub(super) fn enable_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<EnableDisableArgs, _, _, _>(
        CommandSpec::from_args::<EnableDisableArgs>("enable", "Enable an application on a store")
            .with_long(
                "Make a GoDaddy developer-platform application available on a \
                specific store. Use `gddy platform app disable` to reverse this. \
                Both the application name and a store ID are required.",
            )
            .with_system("applications")
            .with_tier(Tier::Mutate)
            .with_scopes(&[APP_REGISTRY_READ, APP_REGISTRY_WRITE])
            .with_output_schema::<ApplicationRef>(),
        |ctx, args: EnableDisableArgs| async move {
            let name = args.name;
            let store_id = args.store_id;
            let client = super::make_client(&ctx).await?;
            let data = client
                .enable_application(json!({ "applicationName": name, "storeId": store_id }))
                .await
                .map_err(super::client_err)?;
            Ok(
                CommandResult::new(data["enableStoreApplication"].clone()).with_next_actions(vec![
                    next_action(
                        "platform app disable <name> --store-id <store-id>",
                        "Disable the application on the same store",
                    )
                    .with_param("name", required_value(&name))
                    .with_param("store-id", required_value(&store_id)),
                    next_action(
                        "platform app info --name <name>",
                        "Inspect application status",
                    )
                    .with_param("name", required_value(&name)),
                ]),
            )
        },
    )
}

pub(super) fn disable_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<EnableDisableArgs, _, _, _>(
        CommandSpec::from_args::<EnableDisableArgs>("disable", "Disable an application on a store")
            .with_long(
                "Remove a GoDaddy developer-platform application from a specific \
                store, making it unavailable there. Use `gddy platform app enable` \
                to re-enable it. Both the application name and a store ID are \
                required.",
            )
            .with_system("applications")
            .with_tier(Tier::Mutate)
            .with_scopes(&[APP_REGISTRY_READ, APP_REGISTRY_WRITE])
            .with_output_schema::<ApplicationRef>(),
        |ctx, args: EnableDisableArgs| async move {
            let name = args.name;
            let store_id = args.store_id;
            let client = super::make_client(&ctx).await?;
            let data = client
                .disable_application(json!({ "applicationName": name, "storeId": store_id }))
                .await
                .map_err(super::client_err)?;
            Ok(
                CommandResult::new(data["disableStoreApplication"].clone()).with_next_actions(
                    vec![
                        next_action(
                            "platform app enable <name> --store-id <store-id>",
                            "Re-enable the application on the same store",
                        )
                        .with_param("name", required_value(&name))
                        .with_param("store-id", required_value(&store_id)),
                        next_action(
                            "platform app info --name <name>",
                            "Inspect application status",
                        )
                        .with_param("name", required_value(&name)),
                    ],
                ),
            )
        },
    )
}

pub(super) fn archive_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<ValidateArgs, _, _, _>(
        CommandSpec::from_args::<ValidateArgs>("archive", "Archive an application")
            .with_long(
                "Archive a GoDaddy developer-platform application by name. \
                Archiving is irreversible via this CLI; the application will no \
                longer be active. Use `gddy platform app list` to confirm the \
                application name before archiving.",
            )
            .with_system("applications")
            .with_tier(Tier::Destructive)
            .with_scopes(&[APP_REGISTRY_READ, APP_REGISTRY_WRITE])
            .with_output_schema::<ApplicationArchive>(),
        |ctx, args: ValidateArgs| async move {
            let name = args.name;
            let client = super::make_client(&ctx).await?;
            let app_data = client
                .get_application(&name)
                .await
                .map_err(super::client_err)?;
            let app_id = app_data["application"]["id"]
                .as_str()
                .ok_or_else(|| {
                    crate::error::GddyError::not_found(format!("application '{name}' not found"))
                        .into_cli_error()
                })?
                .to_owned();
            let data = client
                .archive_application(&app_id)
                .await
                .map_err(super::client_err)?;
            Ok(
                CommandResult::new(data["archiveApplication"].clone()).with_next_actions(vec![
                    next_action(
                        "platform app info --name <name>",
                        "Inspect archived application",
                    )
                    .with_param("name", required_value(&name)),
                    next_action("platform app list", "List all platform apps"),
                ]),
            )
        },
    )
}
