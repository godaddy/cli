//! `gddy platform app config` — inspect the local godaddy.toml manifest.

use cli_engine::{
    CommandResult, CommandSpec, GroupSpec, RuntimeCommandSpec, RuntimeGroupSpec, Tier,
};
use serde_json::json;

use super::schemas::ValidationResult;
use crate::config::ConfigError;
use crate::next_action::{next_action, required_value};

pub(super) fn group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(GroupSpec::new(
        "config",
        "Inspect the local godaddy.toml manifest",
    ))
    .with_command(RuntimeCommandSpec::new_with_context(
        CommandSpec::new("validate", "Validate the local godaddy.toml manifest")
            .with_long(
                "Read the godaddy.toml (or godaddy.<env>.toml, per --env) \
                manifest in the current directory and validate it against the \
                same rules enforced when writing it via `platform app add`/ \
                `init`/`release` — required fields, URL/UUID/semver shapes, \
                and settings placement rules (slug/entry-path/capability \
                enum). Reports every violation found, not just the first.",
            )
            .with_system("applications")
            .with_tier(Tier::Read)
            .with_output_schema::<ValidationResult>()
            .no_auth(true),
        |ctx| async move {
            let path = crate::config::config_path(Some(&ctx.middleware.env));
            match crate::config::read_config(&path) {
                Ok(config) => Ok(CommandResult::new(json!({
                    "valid": true,
                    "errors": Vec::<String>::new(),
                    "warnings": Vec::<String>::new(),
                }))
                .with_next_actions(vec![
                    next_action(
                        "platform app info --name <name>",
                        "Inspect the registered application",
                    )
                    .with_param("name", required_value(&config.name)),
                ])),
                Err(ConfigError::Validation(message)) => Ok(CommandResult::new(json!({
                    "valid": false,
                    "errors": message.split("; ").map(str::to_owned).collect::<Vec<_>>(),
                    "warnings": Vec::<String>::new(),
                }))),
                Err(e) => Err(crate::error::GddyError::config(e.to_string()).into_cli_error()),
            }
        },
    ))
}
