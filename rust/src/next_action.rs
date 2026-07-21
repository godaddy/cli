//! Helper so every suggested next-step command reads exactly as the user would
//! type it (e.g. `gddy domain quote <domain>`, not `domain quote <domain>`).
//!
//! `cli_engine::NextAction` is a generic, app-agnostic type with no concept of a
//! binary name, so the `gddy` prefix is applied here in the main repo rather than
//! in the (separately versioned) `cli-engine` crate.

use cli_engine::{NextAction, NextActionParam};

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

#[cfg(test)]
mod tests {
    use super::required_value;

    #[test]
    fn required_value_sets_value_and_required() {
        let param = required_value("my-app");
        assert_eq!(param.value.as_deref(), Some("my-app"));
        assert!(param.required);
    }
}
