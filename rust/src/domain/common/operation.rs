use cli_engine::{NextAction, NextActionParam};
use domains_client::types;

use crate::next_action::next_action;

/// Points at the completed order, which is where the total actually charged
/// (including tax) can be read — the registration and operation records only
/// carry the quoted price. `--wait` because a new order takes a few seconds to
/// become visible.
pub(crate) fn order_next_action(order_id: String) -> NextAction {
    next_action(
        "shopping order get <order-id> --wait",
        "See the total charged, including any taxes and fees",
    )
    .with_param("order-id", NextActionParam::value(order_id))
}

/// Whether an async domain-operation status is terminal (no further polling).
/// Only `COMPLETED`/`FAILED` are terminal; every other status (e.g. `SUBMITTED`,
/// `PENDING`, `CONFIRMED`, `EXECUTING`) is treated as still in progress.
pub(crate) fn is_terminal_status(status: &str) -> bool {
    matches!(status, "COMPLETED" | "FAILED")
}

/// Renders a `FAILED` operation's `error` payload (name/message) for the
/// user-facing error, e.g. `" (DOMAIN_UNAVAILABLE: the domain is already
/// registered)"`. Empty when the operation carried no error detail (e.g. it
/// failed before an operation with a populated `error` was ever polled).
pub(crate) fn format_operation_error(error: Option<&types::Error>) -> String {
    let Some(error) = error else {
        return String::new();
    };
    match (error.name.as_deref(), error.message.as_deref()) {
        (Some(name), Some(message)) => format!(" ({name}: {message})"),
        (Some(name), None) => format!(" ({name})"),
        (None, Some(message)) => format!(" ({message})"),
        (None, None) => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_next_action_uses_a_structured_param_and_waits_for_the_order() {
        let action = order_next_action("2211623318".to_owned());
        assert_eq!(action.command, "gddy shopping order get <order-id> --wait");
        assert_eq!(
            action.params["order-id"].value.as_deref(),
            Some("2211623318")
        );
        assert!(action.description.contains("taxes and fees"));
    }

    #[test]
    fn terminal_status_detection() {
        assert!(is_terminal_status("COMPLETED"));
        assert!(is_terminal_status("FAILED"));
        assert!(!is_terminal_status("CONFIRMED"));
        assert!(!is_terminal_status("EXECUTING"));
        assert!(!is_terminal_status("SUBMITTED"));
    }

    #[test]
    fn format_operation_error_includes_name_and_message() {
        assert_eq!(format_operation_error(None), "");

        let name_only = types::Error {
            name: Some("DOMAIN_UNAVAILABLE".to_string()),
            ..Default::default()
        };
        assert_eq!(
            format_operation_error(Some(&name_only)),
            " (DOMAIN_UNAVAILABLE)"
        );

        let message_only = types::Error {
            message: Some("the domain is already registered".to_string()),
            ..Default::default()
        };
        assert_eq!(
            format_operation_error(Some(&message_only)),
            " (the domain is already registered)"
        );

        let both = types::Error {
            name: Some("DOMAIN_UNAVAILABLE".to_string()),
            message: Some("the domain is already registered".to_string()),
            ..Default::default()
        };
        assert_eq!(
            format_operation_error(Some(&both)),
            " (DOMAIN_UNAVAILABLE: the domain is already registered)"
        );

        let neither = types::Error::default();
        assert_eq!(format_operation_error(Some(&neither)), "");
    }
}
