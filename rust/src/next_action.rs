//! Helpers for app-specific next-action construction.
//!
//! `cli_engine::NextAction` owns structured parameters and all human rendering;
//! this module only adds the application binary name to command templates.

use cli_engine::{NextAction, NextActionParam};

use crate::environments::APP_ID;

pub(crate) fn next_action(
    command: impl Into<String>,
    description: impl Into<String>,
) -> NextAction {
    NextAction::new(format!("{APP_ID} {}", command.into()), description)
}

/// Prefill a required next-action parameter.
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
