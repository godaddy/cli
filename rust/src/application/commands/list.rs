//! `gddy platform app list` — list all applications visible to the account.

use cli_engine::{CommandResult, CommandSpec, NextActionParam, RuntimeCommandSpec, Tier};

use super::schemas::ApplicationSummary;
use crate::next_action::next_action;

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_with_context(
        CommandSpec::new("list", "List all applications")
            .with_long(
                "List all GoDaddy developer-platform applications visible to \
                the current account. Use `gddy platform app info --name <name>` \
                to fetch full details for a single application.",
            )
            .with_system("applications")
            .with_tier(Tier::Read)
            .with_default_fields("name,label,status")
            .with_output_schema::<ApplicationSummary>(),
        |ctx| async move {
            let client = super::make_client(&ctx).await?;
            let data = client
                .list_applications()
                .await
                .map_err(super::client_err)?;
            Ok(CommandResult::new(data).with_next_actions(vec![
                next_action(
                    "platform app info --name <name>",
                    "Get details for a specific application",
                )
                .with_param("name", NextActionParam::required()),
                next_action(
                    "platform app init --name <name> --description <description> --url <url> --proxy-url <proxy-url> --scopes <scopes>",
                    "Initialize a new application",
                ),
            ]))
        },
    )
}

#[cfg(test)]
mod tests {
    use cli_engine::{Cli, CliConfig, Stage};

    /// API commands must stay fail-closed: `platform app list` calls the backend,
    /// so it must require authentication. Built with **no auth provider
    /// registered**, the engine's default `AuthRequirement::Required` must reject
    /// it before the handler runs (no network call). This guards against someone
    /// mistakenly marking an API command `no_auth(true)`, which would let it run
    /// unauthenticated.
    #[tokio::test]
    async fn platform_app_list_requires_auth() {
        let cli = Cli::new(
            CliConfig::new("gddy", "GoDaddy developer CLI", "gddy")
                .with_min_stage(Stage::Experimental)
                .with_default_auth_provider("godaddy")
                .with_module(crate::platform::module()),
        );

        let output = cli
            .run(["gddy", "platform", "app", "list", "--output", "json"])
            .await;

        // The engine maps auth-resolution failures to exit code 2; together with
        // the provider-named error below this proves the command was rejected at
        // credential resolution, not by the handler hitting the network.
        const AUTH_FAILURE_EXIT: i32 = 2;
        assert_eq!(
            output.exit_code, AUTH_FAILURE_EXIT,
            "platform app list must fail closed at auth resolution, got: {}",
            output.rendered
        );
        let json: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json output");
        let message = json["error"]["message"].as_str().unwrap_or_default();
        assert!(
            message.contains("provider"),
            "expected an auth-provider resolution error, got: {}",
            output.rendered
        );
    }
}
