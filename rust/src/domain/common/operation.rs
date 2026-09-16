use domains_client::types;

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
