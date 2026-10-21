use std::borrow::Cow;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::io::{self, IsTerminal};

#[cfg(test)]
use std::io::{BufRead, Write};

use cli_engine::{CliCoreError, DetailedError};

use super::client::OnboardingClient;
use super::flow::{AgreementDecision, decide};
use super::prompt::prompt_accept_agreements;
use crate::environments;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnsureOutcome {
    pub org_id: String,
    /// `false` when the account was already set up and nothing was submitted.
    pub newly_onboarded: bool,
}

/// What [`ensure_ready_for_app_init`] would do for the current account,
/// determined without submitting anything.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OnboardingPreview {
    AlreadyComplete { org_id: String },
    WouldComplete { org_id: String },
}

#[derive(Debug)]
pub(crate) struct AgreementsRequiredError;

impl Display for AgreementsRequiredError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(
            "GoDaddy Developer agreements must be accepted before creating an application. \
             Re-run interactively, or pass --accept-agreements.",
        )
    }
}

impl Error for AgreementsRequiredError {}

impl DetailedError for AgreementsRequiredError {
    fn error_code(&self) -> Cow<'static, str> {
        Cow::Borrowed("AGREEMENTS_REQUIRED")
    }

    fn error_system(&self) -> Option<Cow<'static, str>> {
        Some(Cow::Borrowed("applications"))
    }

    fn error_request_id(&self) -> Option<Cow<'static, str>> {
        None
    }
}

/// Ensure the authenticated customer has an active org before creating an
/// application or a native-app record.
///
/// Prompts only for `PENDING` users in a terminal who did not pass
/// `accept_agreements`; passing it is the explicit acceptance, so it never
/// prompts. Does not run during `auth login`.
pub async fn ensure_ready_for_app_init(
    token: &str,
    env: &str,
    accept_agreements: bool,
) -> Result<EnsureOutcome, CliCoreError> {
    let base_url = devx_core_base_url(env)?;

    ensure_ready_for_app_init_at(
        token,
        &base_url,
        accept_agreements,
        io::stdin().is_terminal(),
    )
    .await
}

fn devx_core_base_url(env: &str) -> Result<String, CliCoreError> {
    environments::devx_core_url(env).ok_or_else(|| missing_service_url_error(env))
}

fn missing_service_url_error(env: &str) -> CliCoreError {
    let prefix = environments::env_prefix(env);
    crate::error::GddyError::config(format!(
        "Developer account setup isn't configured for the `{env}` environment."
    ))
    .with_fix(format!(
        "Set {prefix}_DEVX_CORE_URL (or DEVX_CORE_URL) to the developer onboarding URL for this environment, then retry."
    ))
    .with_system("applications")
    .into_cli_error()
}

/// Report what [`ensure_ready_for_app_init`] would do, without prompting or
/// submitting anything. Fails exactly where the real call would on an account
/// that cannot be onboarded.
pub async fn preview_onboarding(token: &str, env: &str) -> Result<OnboardingPreview, CliCoreError> {
    preview_onboarding_at(token, &devx_core_base_url(env)?).await
}

pub(crate) async fn preview_onboarding_at(
    token: &str,
    base_url: &str,
) -> Result<OnboardingPreview, CliCoreError> {
    let status = OnboardingClient::new(base_url)
        .status(token)
        .await
        .map_err(|error| {
            CliCoreError::message(format!(
                "Could not check developer onboarding status: {error}"
            ))
        })?;

    match status.status.as_str() {
        "ACTIVE" => Ok(OnboardingPreview::AlreadyComplete {
            org_id: status.org_id,
        }),
        "PENDING" => Ok(OnboardingPreview::WouldComplete {
            org_id: status.org_id,
        }),
        _ => Err(unsupported_status_error(&status)),
    }
}

fn unsupported_status_error(status: &super::types::OnboardingStatus) -> CliCoreError {
    CliCoreError::message(format!(
        "Developer organization status `{}` cannot create applications yet.",
        status.status
    ))
}

/// Same gate as [`ensure_ready_for_app_init`], against an already resolved DevX
/// Core URL and a caller-supplied terminal state.
pub(crate) async fn ensure_ready_for_app_init_at(
    token: &str,
    base_url: &str,
    accept_agreements: bool,
    is_tty: bool,
) -> Result<EnsureOutcome, CliCoreError> {
    let client = OnboardingClient::new(base_url);
    let status = client.status(token).await.map_err(|error| {
        CliCoreError::message(format!(
            "Could not check developer onboarding status before creating an application: {error}"
        ))
    })?;

    let prompt_accepted = if status.status == "PENDING" && is_tty && !accept_agreements {
        // Keep the stdin lock off the async path so the command future stays `Send`.
        let prompt_result = {
            let mut stdin = io::stdin().lock();
            let mut stderr = io::stderr();
            prompt_accept_agreements(&mut stdin, &mut stderr)
        };
        Some(prompt_result.map_err(prompt_error)?)
    } else {
        None
    };

    finish_decision(
        client,
        token,
        &status,
        is_tty,
        accept_agreements,
        prompt_accepted,
    )
    .await
}

#[cfg(test)]
pub(crate) async fn ensure_ready_for_app_init_with(
    token: &str,
    base_url: &str,
    accept_agreements: bool,
    is_tty: bool,
    reader: &mut impl BufRead,
    stderr: &mut impl Write,
) -> Result<EnsureOutcome, CliCoreError> {
    let client = OnboardingClient::new(base_url);
    let status = client.status(token).await.map_err(|error| {
        CliCoreError::message(format!(
            "Could not check developer onboarding status before creating an application: {error}"
        ))
    })?;

    let prompt_accepted = if status.status == "PENDING" && is_tty && !accept_agreements {
        Some(prompt_accept_agreements(reader, stderr).map_err(prompt_error)?)
    } else {
        None
    };

    finish_decision(
        client,
        token,
        &status,
        is_tty,
        accept_agreements,
        prompt_accepted,
    )
    .await
}

fn prompt_error(error: io::Error) -> CliCoreError {
    CliCoreError::message(format!(
        "Could not collect developer agreement acceptance: {error}"
    ))
}

async fn finish_decision(
    client: OnboardingClient,
    token: &str,
    status: &super::types::OnboardingStatus,
    is_tty: bool,
    accept_agreements: bool,
    prompt_accepted: Option<bool>,
) -> Result<EnsureOutcome, CliCoreError> {
    match decide(status, is_tty, accept_agreements, prompt_accepted) {
        AgreementDecision::AlreadyComplete { org_id } => Ok(EnsureOutcome {
            org_id,
            newly_onboarded: false,
        }),
        AgreementDecision::CompletePending => {
            let result = client.complete(token).await.map_err(|error| {
                CliCoreError::message(format!(
                    "Could not complete developer onboarding before creating an application: {error}"
                ))
            })?;
            Ok(EnsureOutcome {
                org_id: result.organization_id,
                newly_onboarded: true,
            })
        }
        AgreementDecision::AgreementsRequired => {
            Err(CliCoreError::with_detailed_error(AgreementsRequiredError))
        }
        AgreementDecision::UnsupportedStatus => Err(unsupported_status_error(status)),
    }
}

#[cfg(test)]
mod tests {
    use std::io::{self, Cursor, Write};

    use httpmock::{Method::POST, MockServer};
    use serde_json::json;

    use super::{
        EnsureOutcome, OnboardingPreview, ensure_ready_for_app_init_with, preview_onboarding_at,
    };

    struct FailingWriter;

    impl Write for FailingWriter {
        fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
            Err(io::Error::other("writer unavailable"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Err(io::Error::other("writer unavailable"))
        }
    }

    #[tokio::test]
    async fn active_status_is_ready_without_prompt_or_cli() {
        let server = MockServer::start_async().await;
        let status = server
            .mock_async(|when, then| {
                when.method(POST).path("/api/v1/onboarding/status");
                then.status(200).json_body(json!({
                    "success": true,
                    "data": { "id": "org-active", "status": "ACTIVE" }
                }));
            })
            .await;
        let complete = server
            .mock_async(|when, then| {
                when.method(POST).path("/api/v1/onboarding/cli");
                then.status(200).json_body(json!({
                    "success": true,
                    "data": { "organizationId": "org-active", "status": "ACTIVE" }
                }));
            })
            .await;

        let mut input = Cursor::new(b"");
        let mut stderr = Vec::new();
        let outcome = ensure_ready_for_app_init_with(
            "test-token",
            &server.base_url(),
            false,
            true,
            &mut input,
            &mut stderr,
        )
        .await
        .expect("active ready");

        assert_eq!(
            outcome,
            EnsureOutcome {
                org_id: "org-active".to_owned(),
                newly_onboarded: false,
            }
        );
        assert!(stderr.is_empty());
        status.assert_async().await;
        assert_eq!(complete.calls_async().await, 0);
    }

    #[tokio::test]
    async fn pending_non_tty_without_flag_requires_agreements() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(POST).path("/api/v1/onboarding/status");
                then.status(200).json_body(json!({
                    "success": true,
                    "data": { "id": "org-pending", "status": "PENDING" }
                }));
            })
            .await;
        let complete = server
            .mock_async(|when, then| {
                when.method(POST).path("/api/v1/onboarding/cli");
                then.status(200).json_body(json!({
                    "success": true,
                    "data": { "organizationId": "org-pending", "status": "ACTIVE" }
                }));
            })
            .await;

        let err = ensure_ready_for_app_init_with(
            "test-token",
            &server.base_url(),
            false,
            false,
            &mut Cursor::new(b""),
            &mut Vec::new(),
        )
        .await
        .expect_err("agreements required");

        assert!(err.to_string().contains("agreements must be accepted"));
        assert_eq!(complete.calls_async().await, 0);
    }

    #[tokio::test]
    async fn pending_non_tty_with_flag_completes() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(POST).path("/api/v1/onboarding/status");
                then.status(200).json_body(json!({
                    "success": true,
                    "data": { "id": "org-pending", "status": "PENDING" }
                }));
            })
            .await;
        let complete = server
            .mock_async(|when, then| {
                when.method(POST).path("/api/v1/onboarding/cli");
                then.status(200).json_body(json!({
                    "success": true,
                    "data": { "organizationId": "org-pending", "status": "ACTIVE" }
                }));
            })
            .await;

        let outcome = ensure_ready_for_app_init_with(
            "test-token",
            &server.base_url(),
            true,
            false,
            &mut Cursor::new(b""),
            &mut Vec::new(),
        )
        .await
        .expect("completed");

        assert_eq!(outcome.org_id, "org-pending");
        assert!(outcome.newly_onboarded);
        complete.assert_async().await;
    }

    #[tokio::test]
    async fn pending_tty_enter_completes_with_prompt() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(POST).path("/api/v1/onboarding/status");
                then.status(200).json_body(json!({
                    "success": true,
                    "data": { "id": "org-pending", "status": "PENDING" }
                }));
            })
            .await;
        let complete = server
            .mock_async(|when, then| {
                when.method(POST).path("/api/v1/onboarding/cli");
                then.status(200).json_body(json!({
                    "success": true,
                    "data": { "organizationId": "org-pending", "status": "ACTIVE" }
                }));
            })
            .await;

        let mut stderr = Vec::new();
        let outcome = ensure_ready_for_app_init_with(
            "test-token",
            &server.base_url(),
            false,
            true,
            &mut Cursor::new(b"\n"),
            &mut stderr,
        )
        .await
        .expect("completed");

        assert_eq!(outcome.org_id, "org-pending");
        assert!(outcome.newly_onboarded);
        let prompt = String::from_utf8(stderr).expect("utf8");
        assert!(prompt.contains("terms-of-use"));
        complete.assert_async().await;
    }

    #[tokio::test]
    async fn pending_tty_with_flag_completes_without_prompting() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(POST).path("/api/v1/onboarding/status");
                then.status(200).json_body(json!({
                    "success": true,
                    "data": { "id": "org-pending", "status": "PENDING" }
                }));
            })
            .await;
        let complete = server
            .mock_async(|when, then| {
                when.method(POST).path("/api/v1/onboarding/cli");
                then.status(200).json_body(json!({
                    "success": true,
                    "data": { "organizationId": "org-pending", "status": "ACTIVE" }
                }));
            })
            .await;

        let mut stderr = Vec::new();
        let outcome = ensure_ready_for_app_init_with(
            "test-token",
            &server.base_url(),
            true,
            true,
            &mut Cursor::new(b""),
            &mut stderr,
        )
        .await
        .expect("completed");

        assert_eq!(outcome.org_id, "org-pending");
        assert!(outcome.newly_onboarded);
        assert!(stderr.is_empty(), "the flag must replace the prompt");
        complete.assert_async().await;
    }

    #[tokio::test]
    async fn pending_tty_reports_prompt_io_errors() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(POST).path("/api/v1/onboarding/status");
                then.status(200).json_body(json!({
                    "success": true,
                    "data": { "id": "org-pending", "status": "PENDING" }
                }));
            })
            .await;
        let complete = server
            .mock_async(|when, then| {
                when.method(POST).path("/api/v1/onboarding/cli");
                then.status(200).json_body(json!({
                    "success": true,
                    "data": { "organizationId": "org-pending", "status": "ACTIVE" }
                }));
            })
            .await;

        let err = ensure_ready_for_app_init_with(
            "test-token",
            &server.base_url(),
            false,
            true,
            &mut Cursor::new(b"\n"),
            &mut FailingWriter,
        )
        .await
        .expect_err("prompt I/O error must be reported");

        assert!(err.to_string().contains("writer unavailable"));
        assert_eq!(complete.calls_async().await, 0);
    }

    /// Serves `/status` with `status` and a `/cli` mock the caller can check
    /// was never hit.
    async fn mock_account<'a>(server: &'a MockServer, status: &str) -> httpmock::Mock<'a> {
        server
            .mock_async(|when, then| {
                when.method(POST).path("/api/v1/onboarding/status");
                then.status(200).json_body(json!({
                    "success": true,
                    "data": { "id": "org-1", "status": status }
                }));
            })
            .await;
        server
            .mock_async(|when, then| {
                when.method(POST).path("/api/v1/onboarding/cli");
                then.status(200).json_body(json!({
                    "success": true,
                    "data": { "organizationId": "org-1", "status": "ACTIVE" }
                }));
            })
            .await
    }

    #[tokio::test]
    async fn preview_reports_a_pending_account_without_submitting() {
        let server = MockServer::start_async().await;
        let complete = mock_account(&server, "PENDING").await;

        let preview = preview_onboarding_at("test-token", &server.base_url())
            .await
            .expect("preview");

        assert_eq!(
            preview,
            OnboardingPreview::WouldComplete {
                org_id: "org-1".to_owned()
            }
        );
        assert_eq!(complete.calls_async().await, 0);
    }

    #[tokio::test]
    async fn preview_reports_an_active_account_as_already_complete() {
        let server = MockServer::start_async().await;
        let complete = mock_account(&server, "ACTIVE").await;

        let preview = preview_onboarding_at("test-token", &server.base_url())
            .await
            .expect("preview");

        assert_eq!(
            preview,
            OnboardingPreview::AlreadyComplete {
                org_id: "org-1".to_owned()
            }
        );
        assert_eq!(complete.calls_async().await, 0);
    }

    #[tokio::test]
    async fn preview_fails_where_the_real_call_would_for_other_statuses() {
        let server = MockServer::start_async().await;
        let complete = mock_account(&server, "SUSPENDED").await;

        let err = preview_onboarding_at("test-token", &server.base_url())
            .await
            .expect_err("suspended accounts cannot be onboarded");

        assert!(err.to_string().contains("`SUSPENDED`"));
        assert_eq!(complete.calls_async().await, 0);
    }

    #[test]
    fn missing_service_url_is_a_config_error_with_a_fix() {
        let err = super::missing_service_url_error("test");

        assert!(err.to_string().contains("`test` environment"));
        let envelope = cli_engine::build_error_envelope(&err, "applications");
        assert_eq!(
            envelope.error.as_ref().map(|e| e.code.as_str()),
            Some("CONFIG_ERROR")
        );
        let fix = envelope.fix.expect("fix");
        assert!(fix.contains("TEST_DEVX_CORE_URL"), "got {fix}");
    }
}
