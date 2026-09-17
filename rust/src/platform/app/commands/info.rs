//! `gddy platform app info` — fetch full details for a single application.

use cli_engine::{CommandResult, CommandSpec, NextActionParam, RuntimeCommandSpec, Tier};

use super::schemas::ApplicationSummary;
use crate::next_action::{next_action, required_value};

#[derive(Debug, Clone, clap::Args)]
struct InfoArgs {
    /// Application name.
    #[arg(long, short = 'n', value_name = "NAME")]
    name: String,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<InfoArgs, _, _, _>(
        CommandSpec::from_args::<InfoArgs>("info", "Get application details")
            .with_long(
                "Fetch full details for a single GoDaddy developer-platform \
                application by name, including status, URLs, and authorization \
                scopes. Use `gddy platform app list` to find available names.",
            )
            .with_system("applications")
            .with_tier(Tier::Read)
            .with_output_schema::<ApplicationSummary>(),
        |ctx, args: InfoArgs| async move {
            let name = args.name;
            let client = super::make_client(&ctx).await?;
            let data = client
                .get_application(&name)
                .await
                .map_err(super::client_err)?;
            let app = &data["application"];
            if app.is_null() {
                return Err(crate::error::GddyError::not_found(format!(
                    "application '{name}' not found"
                ))
                .into_cli_error());
            }
            let app_id = app["id"].as_str().unwrap_or("").to_owned();
            Ok(CommandResult::new(app.clone()).with_next_actions(vec![
                next_action(
                    "platform app validate <name>",
                    "Validate application configuration",
                )
                .with_param("name", required_value(&name)),
                next_action(
                    "platform app update --id <id> [--label <label>] [--description <description>]",
                    "Update application configuration",
                )
                .with_param("id", required_value(&app_id)),
                next_action(
                    "platform app release --application-id <application-id> --version <version>",
                    "Create a release",
                )
                .with_param("application-id", required_value(&app_id))
                .with_param("version", NextActionParam::required()),
                next_action(
                    "platform app deploy --name <name>",
                    "Deploy this application",
                )
                .with_param("name", required_value(&name)),
            ]))
        },
    )
}
