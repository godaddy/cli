//! Shared GoDaddy CLI error codes and recovery hints for agent envelopes.
//!
//! Prefer these constructors over bare [`cli_engine::CliCoreError::message`] so
//! JSON envelopes (and streaming deploy error events) get stable `error.code`
//! values and a top-level `fix` recovery hint.

use std::borrow::Cow;
use std::fmt;

use cli_engine::{CliCoreError, DetailedError};

/// Stable agent-facing error codes.
pub(crate) mod codes {
    pub(crate) const NOT_FOUND: &str = "NOT_FOUND";
    pub(crate) const AMBIGUOUS_MATCH: &str = "AMBIGUOUS_MATCH";
    pub(crate) const VALIDATION_ERROR: &str = "VALIDATION_ERROR";
    pub(crate) const NETWORK_ERROR: &str = "NETWORK_ERROR";
    pub(crate) const AUTH_REQUIRED: &str = "AUTH_REQUIRED";
    pub(crate) const CONFIG_ERROR: &str = "CONFIG_ERROR";
    pub(crate) const SECURITY_BLOCKED: &str = "SECURITY_BLOCKED";
    pub(crate) const UNEXPECTED_ERROR: &str = "UNEXPECTED_ERROR";
}

mod fixes {
    pub(super) const NOT_FOUND: &str =
        "Use discovery commands such as: gddy platform app list or gddy platform actions list.";
    pub(super) const AMBIGUOUS_MATCH: &str =
        "Narrow the query, or add --method, to match exactly one operation.";
    pub(super) const VALIDATION: &str = "Review command arguments and try again with valid values.";
    pub(super) const AUTH: &str = "Run: gddy auth login";
    pub(super) const FORBIDDEN: &str = "You may lack permission for this resource. Confirm scopes with: gddy auth scopes, or re-authenticate with: gddy auth login";
    pub(super) const CONFIG: &str = "Check your config with: gddy env info";
    pub(super) const SECURITY: &str =
        "Resolve security findings and rerun: gddy platform app deploy --name <name>";
    pub(super) const UNEXPECTED: &str =
        "Run: gddy tree for command discovery and retry with corrected input.";
    pub(super) const UNEXPECTED_RESPONSE: &str =
        "The API returned an unexpected response body. Retry, or report this if it persists.";
    pub(super) const NETWORK_CONNECTIVITY: &str =
        "Verify environment connectivity with: gddy env get and retry.";
    pub(super) const NETWORK_CLIENT: &str = "Check request path/query/body. Inspect error.details.response for API validation feedback.";
    pub(super) const NETWORK_SERVER: &str =
        "The API is currently failing server-side. Retry, or check service health/incidents.";
    pub(super) const NETWORK_GRAPHQL: &str = "Check GraphQL query, variables, and operationName. Inspect error.details.response.errors for resolver/validation details.";
    pub(super) const NOT_FOUND_HOSTING: &str = "Use: gddy hosting nodejs app list";
    /// Live `api call` 404: the requested URL/resource was not found (not a catalog miss).
    pub(super) const NOT_FOUND_API: &str =
        "Check the request path and parameters. Inspect the response body for details.";
}

fn not_found_fix_for(system: &str) -> &'static str {
    match system {
        "hosting" => fixes::NOT_FOUND_HOSTING,
        "api" => fixes::NOT_FOUND_API,
        // applications / unknown → platform discovery (default NOT_FOUND fix)
        _ => fixes::NOT_FOUND,
    }
}

/// Structured CLI error that maps to a coded envelope via [`DetailedError`].
#[derive(Debug, Clone)]
pub(crate) struct GddyError {
    message: String,
    code: &'static str,
    fix: Option<String>,
    system: Option<String>,
}

impl GddyError {
    #[must_use]
    pub(crate) fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            code,
            fix: None,
            system: None,
        }
    }

    #[must_use]
    pub(crate) fn with_fix(mut self, fix: impl Into<String>) -> Self {
        let fix = fix.into();
        self.fix = (!fix.is_empty()).then_some(fix);
        self
    }

    #[must_use]
    pub(crate) fn with_system(mut self, system: impl Into<String>) -> Self {
        let system = system.into();
        self.system = (!system.is_empty()).then_some(system);
        self
    }

    #[must_use]
    pub(crate) fn not_found(message: impl Into<String>) -> Self {
        Self::new(codes::NOT_FOUND, message).with_fix(fixes::NOT_FOUND)
    }

    /// Something *was* found, just more than one thing — e.g. a fuzzy
    /// operation query matching several unrelated endpoints, or an exact
    /// path shared by more than one HTTP method with no `--method` given to
    /// pick one. Distinct from [`not_found`](Self::not_found): the caller's
    /// input identified a real ambiguity, not a dead end. Callers
    /// invariably override the default fix with the actual candidate list.
    #[must_use]
    pub(crate) fn ambiguous(message: impl Into<String>) -> Self {
        Self::new(codes::AMBIGUOUS_MATCH, message).with_fix(fixes::AMBIGUOUS_MATCH)
    }

    #[must_use]
    pub(crate) fn validation(message: impl Into<String>) -> Self {
        Self::new(codes::VALIDATION_ERROR, message).with_fix(fixes::VALIDATION)
    }

    #[must_use]
    pub(crate) fn auth(message: impl Into<String>) -> Self {
        Self::new(codes::AUTH_REQUIRED, message).with_fix(fixes::AUTH)
    }

    #[must_use]
    pub(crate) fn config(message: impl Into<String>) -> Self {
        Self::new(codes::CONFIG_ERROR, message).with_fix(fixes::CONFIG)
    }

    #[must_use]
    pub(crate) fn security(message: impl Into<String>) -> Self {
        Self::new(codes::SECURITY_BLOCKED, message).with_fix(fixes::SECURITY)
    }

    #[must_use]
    pub(crate) fn unexpected(message: impl Into<String>) -> Self {
        Self::new(codes::UNEXPECTED_ERROR, message).with_fix(fixes::UNEXPECTED)
    }

    #[must_use]
    pub(crate) fn network(message: impl Into<String>) -> Self {
        Self::new(codes::NETWORK_ERROR, message).with_fix(fixes::NETWORK_CONNECTIVITY)
    }

    /// Map an HTTP status (+ body) to a coded error with a status-aware fix.
    ///
    /// - `401` → auth (missing/expired credentials)
    /// - `403` → network/client (authenticated but not allowed)
    /// - `404` → not found (system-specific discovery fix)
    /// - `2xx` → unexpected (callers sometimes wrap malformed success bodies as Http)
    /// - other `4xx` / `5xx` → network with client/server fixes
    #[must_use]
    pub(crate) fn from_http(status: u16, body: impl AsRef<str>, system: impl Into<String>) -> Self {
        let body = body.as_ref();
        let message = if body.is_empty() {
            format!("HTTP error {status}")
        } else {
            format!("HTTP error {status}: {body}")
        };
        let system = system.into();
        match status {
            401 => Self::auth(message).with_system(system),
            403 => Self::new(codes::NETWORK_ERROR, message)
                .with_fix(fixes::FORBIDDEN)
                .with_system(system),
            404 => Self::new(codes::NOT_FOUND, message)
                .with_fix(not_found_fix_for(&system))
                .with_system(system),
            200..=299 => Self::new(codes::UNEXPECTED_ERROR, message)
                .with_fix(fixes::UNEXPECTED_RESPONSE)
                .with_system(system),
            400..=499 => Self::new(codes::NETWORK_ERROR, message)
                .with_fix(fixes::NETWORK_CLIENT)
                .with_system(system),
            500..=599 => Self::new(codes::NETWORK_ERROR, message)
                .with_fix(fixes::NETWORK_SERVER)
                .with_system(system),
            _ => Self::network(message).with_system(system),
        }
    }

    /// Map GraphQL transport/resolver failures.
    #[must_use]
    pub(crate) fn from_graphql(message: impl Into<String>, system: impl Into<String>) -> Self {
        Self::new(codes::NETWORK_ERROR, message)
            .with_fix(fixes::NETWORK_GRAPHQL)
            .with_system(system)
    }

    /// Convert into a [`CliCoreError`] that carries `code` + top-level `fix`.
    ///
    /// Prefer this over [`.into()`](Into::into) at `Err(...)` call sites —
    /// [`CliCoreError`] has several `From` impls and type inference often fails.
    #[must_use]
    pub(crate) fn into_cli_error(self) -> CliCoreError {
        CliCoreError::with_detailed_error(self)
    }
}

impl fmt::Display for GddyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for GddyError {}

impl DetailedError for GddyError {
    fn error_code(&self) -> Cow<'static, str> {
        Cow::Borrowed(self.code)
    }

    fn error_system(&self) -> Option<Cow<'static, str>> {
        self.system.clone().map(Cow::Owned)
    }

    fn error_request_id(&self) -> Option<Cow<'static, str>> {
        None
    }

    fn error_fix(&self) -> Option<Cow<'static, str>> {
        self.fix.clone().map(Cow::Owned)
    }
}

impl From<GddyError> for CliCoreError {
    fn from(value: GddyError) -> Self {
        value.into_cli_error()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_found_sets_code_and_fix() {
        let err = GddyError::not_found("application 'foo' not found");
        assert_eq!(err.error_code(), codes::NOT_FOUND);
        assert!(
            err.error_fix()
                .is_some_and(|f| f.contains("platform app list")),
            "expected discovery fix, got {:?}",
            err.error_fix()
        );

        let envelope = cli_engine::build_error_envelope(&err.into_cli_error(), "applications");
        assert_eq!(
            envelope.error.as_ref().map(|e| e.code.as_str()),
            Some(codes::NOT_FOUND)
        );
        assert_eq!(
            envelope.error.as_ref().map(|e| e.message.as_str()),
            Some("application 'foo' not found")
        );
        assert!(
            envelope
                .fix
                .as_deref()
                .is_some_and(|f| f.contains("platform app list")),
            "envelope.fix missing: {envelope:?}"
        );
    }

    #[test]
    fn ambiguous_sets_code_and_default_fix_but_is_overridable() {
        let err = GddyError::ambiguous("'order' matches 3 operations");
        assert_eq!(err.error_code(), codes::AMBIGUOUS_MATCH);
        assert!(
            err.error_fix().is_some_and(|f| f.contains("--method")),
            "expected default ambiguity fix, got {:?}",
            err.error_fix()
        );

        let overridden = GddyError::ambiguous("'order' matches 3 operations")
            .with_fix("Run one of: gddy api operation get createOrder");
        assert_eq!(
            overridden.error_fix().as_deref(),
            Some("Run one of: gddy api operation get createOrder")
        );
    }

    #[test]
    fn from_http_maps_auth_and_not_found() {
        let auth = GddyError::from_http(401, "unauthorized", "applications");
        assert_eq!(auth.error_code(), codes::AUTH_REQUIRED);

        let forbidden = GddyError::from_http(403, "denied", "applications");
        assert_eq!(forbidden.error_code(), codes::NETWORK_ERROR);
        assert!(
            forbidden
                .error_fix()
                .is_some_and(|f| f.contains("auth scopes")),
            "{:?}",
            forbidden.error_fix()
        );

        let malformed = GddyError::from_http(
            200,
            "invalid JSON response: eof (body: not-json)",
            "applications",
        );
        assert_eq!(malformed.error_code(), codes::UNEXPECTED_ERROR);
        assert!(
            malformed
                .error_fix()
                .is_some_and(|f| f.contains("unexpected response")),
            "{:?}",
            malformed.error_fix()
        );

        let missing = GddyError::from_http(404, "gone", "applications");
        assert_eq!(missing.error_code(), codes::NOT_FOUND);
        assert!(
            missing
                .error_fix()
                .is_some_and(|f| f.contains("platform app list")),
            "{:?}",
            missing.error_fix()
        );

        let hosting_missing = GddyError::from_http(404, "gone", "hosting");
        assert!(
            hosting_missing
                .error_fix()
                .is_some_and(|f| f.contains("hosting nodejs app list")),
            "{:?}",
            hosting_missing.error_fix()
        );

        let api_missing = GddyError::from_http(404, "gone", "api");
        assert!(
            api_missing
                .error_fix()
                .is_some_and(|f| f.contains("request path")),
            "{:?}",
            api_missing.error_fix()
        );

        let client = GddyError::from_http(422, "bad", "applications");
        assert_eq!(client.error_code(), codes::NETWORK_ERROR);
        assert!(
            client
                .error_fix()
                .is_some_and(|f| f.contains("path/query/body")),
            "{:?}",
            client.error_fix()
        );

        let server = GddyError::from_http(503, "down", "applications");
        assert!(
            server
                .error_fix()
                .is_some_and(|f| f.contains("server-side")),
            "{:?}",
            server.error_fix()
        );
    }

    #[test]
    fn security_blocked_code() {
        let err = GddyError::security("security scan blocked deployment of 'x'");
        let envelope = cli_engine::build_error_envelope(&err.into_cli_error(), "applications");
        assert_eq!(
            envelope.error.as_ref().map(|e| e.code.as_str()),
            Some(codes::SECURITY_BLOCKED)
        );
        assert!(
            envelope
                .fix
                .as_deref()
                .is_some_and(|f| f.contains("platform app deploy")),
            "{:?}",
            envelope.fix
        );
    }

    #[test]
    fn auth_with_fix_can_include_scopes() {
        let err =
            GddyError::auth("missing scopes").with_fix("Run: gddy auth login --scope foo:bar");
        assert_eq!(err.error_code(), codes::AUTH_REQUIRED);
        assert_eq!(
            err.error_fix().as_deref(),
            Some("Run: gddy auth login --scope foo:bar")
        );
    }

    #[test]
    fn unexpected_with_custom_fix_overrides_default() {
        let err = GddyError::unexpected("records without recordId")
            .with_fix("Re-run `gddy dns list example.com --type A --name www`");
        assert_eq!(err.error_code(), codes::UNEXPECTED_ERROR);
        assert!(
            err.error_fix().is_some_and(|f| f.contains("dns list")),
            "{:?}",
            err.error_fix()
        );
    }
}
