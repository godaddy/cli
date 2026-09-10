//! Helper so every suggested next-step command reads exactly as the user would
//! type it (e.g. `gddy domain quote <domain>`, not `domain quote <domain>`).
//!
//! `cli_engine::NextAction` is a generic, app-agnostic type with no concept of a
//! binary name, so the `gddy` prefix is applied here in the main repo rather than
//! in the (separately versioned) `cli-engine` crate.

use cli_engine::{NextAction, NextActionParam};
use serde_json::{Value, json};

use crate::environments::APP_ID;

pub(crate) fn next_action(
    command: impl Into<String>,
    description: impl Into<String>,
) -> NextAction {
    NextAction::new(format!("{APP_ID} {}", command.into()), description)
}

/// Prefill a required next-action param (value + `required: true`).
pub(crate) fn required_value(value: impl Into<String>) -> NextActionParam {
    NextActionParam {
        value: Some(value.into()),
        required: true,
        ..Default::default()
    }
}

/// Mirrors cli-engine placeholder substitution for custom human views. The
/// structured template and parameters remain in the output envelope.
pub(crate) fn display_command(action: &NextAction) -> String {
    action
        .params
        .iter()
        .filter_map(|(name, param)| {
            param
                .value
                .as_ref()
                .map(|value| (format!("<{name}>"), value.as_str()))
        })
        .fold(action.command.clone(), |command, (placeholder, value)| {
            command.replace(&placeholder, value)
        })
}

pub(crate) fn human_next_steps(actions: &[NextAction]) -> Value {
    Value::Array(
        actions
            .iter()
            .map(|action| {
                json!({
                    "command": display_command(action),
                    "description": action.description,
                })
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use cli_engine::NextAction;

    use super::{display_command, required_value};

    #[test]
    fn required_value_sets_value_and_required() {
        let param = required_value("my-app");
        assert_eq!(param.value.as_deref(), Some("my-app"));
        assert!(param.required);
    }

    #[test]
    fn display_command_substitutes_known_parameters() {
        let action = NextAction::new("gddy domain get <domain>", "Get a domain")
            .with_param("domain", cli_engine::NextActionParam::value("example.com"));

        assert_eq!(display_command(&action), "gddy domain get example.com");
    }
}
