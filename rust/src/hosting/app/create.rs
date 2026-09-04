use cli_engine::{CommandResult, CommandSpec, NextActionParam, RuntimeCommandSpec, Tier};
use serde_json::json;

use crate::hosting::common::{client_err, make_client, parse_app_type};
use crate::next_action::next_action;
use crate::scopes::HOSTING_APPLICATION_CREATE as APP_CREATE;

#[derive(Debug, Clone, clap::Args)]
struct AppCreateArgs {
    /// Application type (NODEJS).
    #[arg(long = "app-type", value_name = "TYPE", value_parser = parse_app_type)]
    app_type: String,

    /// Human-readable display name (1–200 characters).
    #[arg(long, value_name = "NAME")]
    name: String,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<AppCreateArgs, _, _, _>(
        CommandSpec::from_args::<AppCreateArgs>("create", "Create a hosting application")
            .with_long(
                "Provision a new hosting application slot. Because no app ID exists \
                 until provisioning completes, this returns an operation ID. \
                 Poll `hosting operation get --operation-id <id>` until \
                 status is COMPLETED or FAILED. The completed operation result \
                 includes the created application with its ID.",
            )
            .with_system("hosting")
            .with_tier(Tier::Mutate)
            .mutates(true)
            .with_scopes(&[APP_CREATE]),
        |ctx, args: AppCreateArgs| async move {
            let app_type = args.app_type;
            let name = args.name;
            let client = make_client(&ctx, &[APP_CREATE]).await?;
            let data = client
                .create_app(&app_type, json!({ "name": name }))
                .await
                .map_err(client_err)?;

            let operation_id = data
                .get("operationId")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_owned();

            let mut poll_action = next_action(
                "hosting operation get --operation-id <operation-id>",
                "Poll app provisioning status",
            )
            .with_param("operation-id", NextActionParam::required());

            if !operation_id.is_empty() {
                poll_action = next_action(
                    "hosting operation get --operation-id <operation-id>",
                    "Poll app provisioning status",
                )
                .with_param("operation-id", NextActionParam::value(operation_id));
            }

            Ok(CommandResult::new(data).with_next_actions(vec![poll_action]))
        },
    )
}
