//! How a failed DNS write is reported. `add`, `set` and `delete` attempt every
//! record and report each outcome, so a failure is carried as text for the
//! per-record breakdown together with what kind of failure it was, and the
//! command's error takes its stable `error.code` from that kind.

use std::fmt;
use std::ops::Deref;

use cli_engine::CliCoreError;

use crate::domain::{ApiFailure, api_failure};
use crate::error::GddyError;

use super::conflicts::duplicate_record_issue;

/// What went wrong with one record's write.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FailureKind {
    /// The record, or its name, is already taken (`RECORD_EXISTS`).
    RecordExists,
    /// The domain has no DNS zone for this account (`ZONE_NOT_FOUND`).
    ZoneNotFound,
    /// The record addressed is not there (`RECORD_NOT_FOUND`).
    RecordNotFound,
    /// The API rejected the record's content (`VALIDATION_ERROR`).
    Validation,
    /// Anything else: server errors, transport failures, a malformed response.
    Other,
}

/// One record's failure: the message shown in the breakdown, and its kind.
/// Derefs to the message so callers can read it as text.
#[derive(Clone, Debug)]
pub(super) struct DnsFailure {
    message: String,
    kind: FailureKind,
}

impl DnsFailure {
    pub(super) fn new(kind: FailureKind, message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            kind,
        }
    }

    /// Classify a failed API call from the response it got. A `DUPLICATE_RECORD`
    /// issue means the record exists, whatever the error's own `name` says.
    pub(super) fn from_api(failure: ApiFailure) -> Self {
        let kind = if duplicate_record_issue(&failure.body) {
            FailureKind::RecordExists
        } else {
            match failure.api_name().as_deref() {
                Some("ZONE_NOT_FOUND") => FailureKind::ZoneNotFound,
                Some("RECORD_NOT_FOUND") => FailureKind::RecordNotFound,
                Some("VALIDATION_ERROR") => FailureKind::Validation,
                _ => FailureKind::Other,
            }
        };
        Self::new(kind, failure.message)
    }

    /// Read a failed API call's response and classify it.
    pub(super) async fn from_error(
        action: &str,
        debug: bool,
        err: domains_client::Error<()>,
    ) -> Self {
        Self::from_api(api_failure(action, debug, err).await)
    }

    pub(super) fn kind(&self) -> FailureKind {
        self.kind
    }
}

impl From<String> for DnsFailure {
    fn from(message: String) -> Self {
        Self::new(FailureKind::Other, message)
    }
}

impl Deref for DnsFailure {
    type Target = str;

    fn deref(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for DnsFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

/// A write that did not fully succeed: the message with its per-record
/// breakdown, and the kind to report it as. Derefs to the message.
#[derive(Debug)]
pub(super) struct MutateFailure {
    message: String,
    kind: FailureKind,
}

impl MutateFailure {
    /// The kind is the failures' shared kind; when they differ there is no one
    /// name for the whole, so it is `Other`.
    pub(super) fn new<'a>(
        message: String,
        failures: impl IntoIterator<Item = &'a DnsFailure>,
    ) -> Self {
        let mut kinds = failures.into_iter().map(DnsFailure::kind);
        let kind = match kinds.next() {
            Some(first) if kinds.all(|k| k == first) => first,
            _ => FailureKind::Other,
        };
        Self { message, kind }
    }

    pub(super) fn into_cli_error(self) -> CliCoreError {
        let message = self.message;
        match self.kind {
            FailureKind::RecordExists => GddyError::record_exists(message),
            FailureKind::ZoneNotFound => GddyError::zone_not_found(message),
            FailureKind::RecordNotFound => GddyError::record_not_found(message),
            FailureKind::Validation => GddyError::validation(message),
            FailureKind::Other => GddyError::unexpected(message).with_fix(
                "Re-run with only the failed value(s), or use gddy dns list to inspect the \
                 current records.",
            ),
        }
        .into_cli_error()
    }
}

impl Deref for MutateFailure {
    type Target = str;

    fn deref(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for MutateFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn code(failure: MutateFailure) -> String {
        match failure.kind {
            FailureKind::RecordExists => "RECORD_EXISTS",
            FailureKind::ZoneNotFound => "ZONE_NOT_FOUND",
            FailureKind::RecordNotFound => "RECORD_NOT_FOUND",
            FailureKind::Validation => "VALIDATION_ERROR",
            FailureKind::Other => "UNEXPECTED_ERROR",
        }
        .to_owned()
    }

    fn failure(status: u16, body: &str) -> ApiFailure {
        ApiFailure {
            body: body.to_owned(),
            message: format!("failed (HTTP {status}): {body}"),
        }
    }

    #[test]
    fn the_api_error_name_picks_the_kind() {
        let kind = |status, body: &str| DnsFailure::from_api(failure(status, body)).kind();
        assert_eq!(
            kind(404, r#"{"name":"ZONE_NOT_FOUND"}"#),
            FailureKind::ZoneNotFound
        );
        assert_eq!(
            kind(404, r#"{"name":"RECORD_NOT_FOUND"}"#),
            FailureKind::RecordNotFound
        );
        assert_eq!(
            kind(422, r#"{"name":"VALIDATION_ERROR"}"#),
            FailureKind::Validation
        );
        assert_eq!(kind(500, "boom"), FailureKind::Other);
        assert_eq!(kind(502, ""), FailureKind::Other);
    }

    #[test]
    fn a_duplicate_record_issue_is_record_exists_whatever_the_name() {
        let body = r#"{"name":"VALIDATION_ERROR","details":[{"issue":"DUPLICATE_RECORD"}]}"#;
        assert_eq!(
            DnsFailure::from_api(failure(422, body)).kind(),
            FailureKind::RecordExists
        );
    }

    #[test]
    fn the_command_error_takes_the_shared_kind_of_its_failures() {
        let exists = DnsFailure::new(FailureKind::RecordExists, "a");
        let missing = DnsFailure::new(FailureKind::ZoneNotFound, "b");
        let other = DnsFailure::from("c".to_owned());

        let all_exist = MutateFailure::new("m".into(), [&exists, &exists]);
        assert_eq!(code(all_exist), "RECORD_EXISTS");
        let mixed = MutateFailure::new("m".into(), [&exists, &missing]);
        assert_eq!(code(mixed), "UNEXPECTED_ERROR");
        let one_of_many = MutateFailure::new("m".into(), [&other]);
        assert_eq!(code(one_of_many), "UNEXPECTED_ERROR");
    }

    #[test]
    fn the_cli_error_carries_the_stable_code_and_a_fix() {
        let failure = MutateFailure::new(
            "already there".into(),
            [&DnsFailure::new(FailureKind::RecordExists, "x")],
        );
        let envelope = cli_engine::build_error_envelope(&failure.into_cli_error(), "domain");
        assert_eq!(
            envelope.error.as_ref().map(|e| e.code.as_str()),
            Some("RECORD_EXISTS")
        );
        assert!(
            envelope
                .fix
                .as_deref()
                .is_some_and(|f| f.contains("dns list")),
            "{:?}",
            envelope.fix
        );
    }
}
