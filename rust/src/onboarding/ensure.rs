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

/// Ensure the authenticated customer has an active org before `application init`.
///
/// Prompts only for `PENDING` users. Does not run during `auth login`.
pub async fn ensure_ready_for_app_init(
    token: &str,
    env: &str,
    accept_agreements: bool,
) -> Result<EnsureOutcome, CliCoreError> {
    let Some(base_url) = environments::devx_core_url(env) else {
        return Err(CliCoreError::message(format!(
            "DevX core URL is not configured for environment `{env}`. Set {}_DEVX_CORE_URL or DEVX_CORE_URL to continue.",
            environments::env_prefix(env),
        )));
    };

    let client = OnboardingClient::new(base_url);
    let status = client.status(token).await.map_err(|error| {
        CliCoreError::message(format!(
            "Could not check developer onboarding status before creating an application: {error}"
        ))
    })?;

    let is_tty = io::stdin().is_terminal();
    let prompt_accepted = if status.status == "PENDING" && is_tty {
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

    let prompt_accepted = if status.status == "PENDING" && is_tty {
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
        AgreementDecision::AlreadyComplete { org_id } => Ok(EnsureOutcome { org_id }),
        AgreementDecision::CompletePending => {
            let result = client.complete(token).await.map_err(|error| {
                CliCoreError::message(format!(
                    "Could not complete developer onboarding before creating an application: {error}"
                ))
            })?;
            Ok(EnsureOutcome {
                org_id: result.organization_id,
            })
        }
        AgreementDecision::AgreementsRequired => {
            Err(CliCoreError::with_detailed_error(AgreementsRequiredError))
        }
        AgreementDecision::UnsupportedStatus => Err(CliCoreError::message(format!(
            "Developer organization status `{}` cannot create applications yet.",
            status.status
        ))),
    }
}

#[cfg(test)]
mod tests {
    use std::io::{self, Cursor, Write};

    use httpmock::{Method::POST, MockServer};
    use serde_json::json;

    use super::{EnsureOutcome, ensure_ready_for_app_init_with};

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
                org_id: "org-active".to_owned()
            }
        );
        assert!(stderr.is_empty());
        status.assert_async().await;
        assert_eq!(complete.hits_async().await, 0);
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
        assert_eq!(complete.hits_async().await, 0);
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
        let prompt = String::from_utf8(stderr).expect("utf8");
        assert!(prompt.contains("terms-of-use"));
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
        assert_eq!(complete.hits_async().await, 0);
    }
}
