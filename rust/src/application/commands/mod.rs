//! `gddy platform app` — register, configure, and deploy GoDaddy
//! developer-platform applications described by a godaddy.toml manifest.

use cli_engine::{GroupSpec, NextAction, NextActionParam, RuntimeGroupSpec};

use crate::application::client::api_url_for_env;
use crate::next_action::{next_action, required_value};

mod add;
mod add_extension;
mod deploy;
mod info;
mod init;
mod lifecycle;
mod list;
mod release;
mod schemas;
mod update;
mod validate;

async fn make_client(
    ctx: &cli_engine::CommandContext,
) -> cli_engine::Result<crate::application::client::ApplicationClient> {
    // Lazily resolve the credential; this triggers the auth flow only for
    // commands that actually call the API.
    let token = ctx.credential().await?.token;
    let base_url = api_url_for_env(&ctx.middleware.env)?;
    Ok(crate::application::client::ApplicationClient::new(
        base_url, token,
    ))
}

fn client_err(e: crate::application::client::ClientError) -> cli_engine::CliCoreError {
    crate::error::GddyError::from(e).into_cli_error()
}

fn validation_err(message: impl Into<String>) -> cli_engine::CliCoreError {
    crate::error::GddyError::validation(message).into_cli_error()
}

/// Next-actions after mutating local godaddy.toml (add action/subscription/extension).
fn add_config_next_actions(app_name: &str) -> Vec<NextAction> {
    let name_param = if app_name.is_empty() {
        NextActionParam::required()
    } else {
        required_value(app_name)
    };
    vec![
        next_action(
            "platform app validate <name>",
            "Validate remote application configuration",
        )
        .with_param("name", name_param),
        next_action(
            "platform app release --application-id <application-id> --version <version>",
            "Create a new release",
        )
        .with_param("application-id", NextActionParam::required())
        .with_param("version", NextActionParam::required()),
    ]
}

pub fn application_group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(
        GroupSpec::new("app", "Manage GoDaddy Platform apps")
            .with_long(
                "Manage GoDaddy developer-platform applications. A GoDaddy application is a \
                developer-platform app described by a godaddy.toml manifest in your working \
                directory. Use `gddy platform app init` to create one, `gddy platform app \
                validate <name>` to check remote application state, and `gddy platform app \
                deploy` to publish it.",
            )
            .with_alias("application"),
    )
    .with_command(list::command())
    .with_command(info::command())
    .with_command(init::command())
    .with_command(validate::command())
    .with_command(update::command())
    .with_command(lifecycle::enable_command())
    .with_command(lifecycle::disable_command())
    .with_command(lifecycle::archive_command())
    .with_command(release::command())
    .with_command(deploy::command())
    .with_group(add::group())
}

#[cfg(test)]
mod tests {
    #[test]
    fn reused_next_action_helpers_have_expected_size() {
        assert_eq!(super::add_config_next_actions("app").len(), 2);
        assert_eq!(super::deploy::deploy_next_actions("app").len(), 3);
    }

    #[test]
    fn add_config_next_actions_skips_empty_name_prefill() {
        let actions = super::add_config_next_actions("");
        let name = &actions[0].params["name"];
        assert!(name.required);
        assert_eq!(name.value.as_deref(), None);
    }
}
