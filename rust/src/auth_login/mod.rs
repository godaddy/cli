pub(crate) mod args;
mod output;

use std::io::{self, BufRead, IsTerminal, Write};
use std::process::ExitCode;

use cli_engine::auth::AuthProvider;
use cli_engine::{Cli, CliRunOutput, Envelope};

use crate::auth::GoDaddyAuthProvider;
use crate::environments;
use crate::onboarding::{AgreementDecision, OnboardingClient, decide, prompt_accept_agreements};

pub(crate) enum FinishLogin {
    Success(Envelope),
    AgreementsRequired(Envelope),
}

pub(crate) async fn finish_login(
    mut envelope: Envelope,
    token: &str,
    base_url: &str,
    accept_agreements: bool,
    is_tty: bool,
    reader: &mut impl BufRead,
    stderr: &mut impl Write,
) -> FinishLogin {
    let client = OnboardingClient::new(base_url);
    let status = match client.status(token).await {
        Ok(status) => status,
        Err(_) => {
            output::apply_failed(&mut envelope);
            return FinishLogin::Success(envelope);
        }
    };

    let prompt_accepted = if status.status == "PENDING" && is_tty {
        Some(prompt_accept_agreements(reader, stderr).unwrap_or(false))
    } else {
        None
    };

    match decide(&status, is_tty, accept_agreements, prompt_accepted) {
        AgreementDecision::AlreadyComplete { org_id } => {
            output::apply_complete(&mut envelope, org_id);
            FinishLogin::Success(envelope)
        }
        AgreementDecision::CompletePending => match client.complete(token).await {
            Ok(result) => {
                output::apply_complete(&mut envelope, result.organization_id);
                FinishLogin::Success(envelope)
            }
            Err(_) => {
                output::apply_failed(&mut envelope);
                FinishLogin::Success(envelope)
            }
        },
        AgreementDecision::AgreementsRequired => {
            FinishLogin::AgreementsRequired(output::agreements_required_envelope())
        }
        AgreementDecision::UnsupportedStatus => {
            output::apply_failed(&mut envelope);
            FinishLogin::Success(envelope)
        }
    }
}

pub async fn run(cli: &Cli, auth_provider: &GoDaddyAuthProvider, args: Vec<String>) -> ExitCode {
    let parsed = args::parse(
        cli,
        &args,
        &cli_engine::default_output_format(environments::APP_ID),
    )
    .expect("dispatch only calls run for auth login");

    let login = cli.run(parsed.engine_args.clone()).await;
    let mut envelope: Envelope = match serde_json::from_str(&login.rendered) {
        Ok(envelope) => envelope,
        Err(_) => return write_fallback(&login),
    };

    if login.exit_code != 0 || envelope.error.is_some() {
        return write_envelope(&parsed.output_format, &envelope, login.exit_code);
    }

    let env = envelope
        .data
        .as_ref()
        .and_then(|data| data.get("env"))
        .and_then(|value| value.as_str())
        .unwrap_or(environments::DEFAULT_ENV)
        .to_owned();

    let credential = match auth_provider.status(&env).await {
        Ok(credential) => credential,
        Err(_) => {
            output::apply_failed(&mut envelope);
            return write_envelope(&parsed.output_format, &envelope, 0);
        }
    };

    let Some(base_url) = environments::devx_core_url(&env) else {
        output::apply_failed(&mut envelope);
        return write_envelope(&parsed.output_format, &envelope, 0);
    };

    let is_tty = io::stdin().is_terminal();
    let mut stdin = io::stdin().lock();
    let mut stderr = io::stderr();

    match finish_login(
        envelope,
        &credential.token,
        &base_url,
        parsed.accept_agreements,
        is_tty,
        &mut stdin,
        &mut stderr,
    )
    .await
    {
        FinishLogin::Success(envelope) => write_envelope(&parsed.output_format, &envelope, 0),
        FinishLogin::AgreementsRequired(envelope) => {
            write_envelope(&parsed.output_format, &envelope, 1)
        }
    }
}

fn write_envelope(format: &str, envelope: &Envelope, exit_code: i32) -> ExitCode {
    match output::render(format, envelope) {
        Ok(rendered) => write_rendered(&rendered, exit_code, &mut io::stdout(), &mut io::stderr()),
        Err(error) => {
            let _ = writeln!(io::stderr(), "{error}");
            ExitCode::from(1)
        }
    }
}

fn write_fallback(login: &CliRunOutput) -> ExitCode {
    write_rendered(
        &login.rendered,
        login.exit_code,
        &mut io::stdout(),
        &mut io::stderr(),
    )
}

fn write_rendered(
    rendered: &str,
    exit_code: i32,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
) -> ExitCode {
    let write_result = if exit_code == 0 {
        stdout.write_all(rendered.as_bytes())
    } else {
        stderr.write_all(rendered.as_bytes())
    };
    if write_result.is_err() {
        return ExitCode::from(1);
    }
    process_exit_code(exit_code)
}

fn process_exit_code(code: i32) -> ExitCode {
    if code == 0 {
        return ExitCode::SUCCESS;
    }
    match u8::try_from(code) {
        Ok(code) if code != 0 => ExitCode::from(code),
        Ok(_) | Err(_) => ExitCode::from(1),
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use cli_engine::{CliRunOutput, Envelope, NextAction};
    use httpmock::{Method::POST, MockServer};
    use serde_json::json;

    use super::{FinishLogin, finish_login, write_rendered};
    use crate::auth_login::output;

    fn login_envelope() -> Envelope {
        Envelope::success(
            json!({
                "provider": "godaddy",
                "env": "ote",
                "identity": "customer",
                "expires_at": "2026-07-20T00:00:00Z",
                "scopes": []
            }),
            "auth",
        )
        .with_next_actions(vec![NextAction::new("gddy auth status", "Check login")])
    }

    fn assert_complete(envelope: &Envelope, org_id: &str) {
        let data = envelope.data.as_ref().expect("data");
        assert_eq!(data["org_id"], org_id);
        assert_eq!(data["onboarding"], "complete");
        assert_eq!(data["provider"], "godaddy");
        assert!(envelope.warnings.is_empty());
    }

    fn assert_failed(envelope: &Envelope) {
        let data = envelope.data.as_ref().expect("data");
        assert_eq!(data["onboarding"], "failed");
        assert_eq!(
            envelope.warnings,
            vec![output::ONBOARDING_WARNING.to_owned()]
        );
    }

    #[tokio::test]
    async fn active_status_completes_without_prompt_or_cli_call() {
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
        let result = finish_login(
            login_envelope(),
            "test-token",
            &server.base_url(),
            false,
            true,
            &mut input,
            &mut stderr,
        )
        .await;

        match result {
            FinishLogin::Success(envelope) => assert_complete(&envelope, "org-active"),
            FinishLogin::AgreementsRequired(_) => panic!("expected success"),
        }
        assert!(stderr.is_empty());
        status.assert_async().await;
        assert_eq!(complete.hits_async().await, 0);
    }

    #[tokio::test]
    async fn pending_tty_enter_prompts_and_completes() {
        let server = MockServer::start_async().await;
        let status = server
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

        let mut input = Cursor::new(b"\n");
        let mut stderr = Vec::new();
        let result = finish_login(
            login_envelope(),
            "test-token",
            &server.base_url(),
            false,
            true,
            &mut input,
            &mut stderr,
        )
        .await;

        match result {
            FinishLogin::Success(envelope) => assert_complete(&envelope, "org-pending"),
            FinishLogin::AgreementsRequired(_) => panic!("expected success"),
        }
        let prompt = String::from_utf8(stderr).expect("utf8");
        assert!(prompt.contains("terms-of-use"));
        assert!(prompt.contains("privacy-policy"));
        assert!(prompt.contains("developer-agreement"));
        status.assert_async().await;
        complete.assert_async().await;
    }

    #[tokio::test]
    async fn pending_non_tty_flag_completes_without_prompt() {
        let server = MockServer::start_async().await;
        let status = server
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

        let mut input = Cursor::new(b"");
        let mut stderr = Vec::new();
        let result = finish_login(
            login_envelope(),
            "test-token",
            &server.base_url(),
            true,
            false,
            &mut input,
            &mut stderr,
        )
        .await;

        match result {
            FinishLogin::Success(envelope) => assert_complete(&envelope, "org-pending"),
            FinishLogin::AgreementsRequired(_) => panic!("expected success"),
        }
        assert!(stderr.is_empty());
        status.assert_async().await;
        complete.assert_async().await;
    }

    #[tokio::test]
    async fn pending_non_tty_without_flag_requires_agreements() {
        let server = MockServer::start_async().await;
        let status = server
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

        let mut input = Cursor::new(b"");
        let mut stderr = Vec::new();
        let result = finish_login(
            login_envelope(),
            "test-token",
            &server.base_url(),
            false,
            false,
            &mut input,
            &mut stderr,
        )
        .await;

        match result {
            FinishLogin::AgreementsRequired(envelope) => {
                let error = envelope.error.expect("structured error");
                assert_eq!(error.code, "AGREEMENTS_REQUIRED");
            }
            FinishLogin::Success(_) => panic!("expected agreements required"),
        }
        assert!(stderr.is_empty());
        status.assert_async().await;
        assert_eq!(complete.hits_async().await, 0);
    }

    #[tokio::test]
    async fn status_http_failure_is_non_fatal() {
        let server = MockServer::start_async().await;
        let status = server
            .mock_async(|when, then| {
                when.method(POST).path("/api/v1/onboarding/status");
                then.status(500).body("secret backend detail");
            })
            .await;
        let complete = server
            .mock_async(|when, then| {
                when.method(POST).path("/api/v1/onboarding/cli");
                then.status(200).json_body(json!({
                    "success": true,
                    "data": { "organizationId": "org-1", "status": "ACTIVE" }
                }));
            })
            .await;

        let mut input = Cursor::new(b"");
        let mut stderr = Vec::new();
        let result = finish_login(
            login_envelope(),
            "test-token",
            &server.base_url(),
            true,
            false,
            &mut input,
            &mut stderr,
        )
        .await;

        match result {
            FinishLogin::Success(envelope) => {
                assert_failed(&envelope);
                let rendered = serde_json::to_string(&envelope).expect("serialize");
                assert!(!rendered.contains("secret backend detail"));
                assert!(!rendered.contains("test-token"));
            }
            FinishLogin::AgreementsRequired(_) => panic!("expected non-fatal success"),
        }
        status.assert_async().await;
        assert_eq!(complete.hits_async().await, 0);
    }

    #[tokio::test]
    async fn completion_http_failure_is_non_fatal() {
        let server = MockServer::start_async().await;
        let status = server
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
                then.status(500).body("secret backend detail");
            })
            .await;

        let mut input = Cursor::new(b"");
        let mut stderr = Vec::new();
        let result = finish_login(
            login_envelope(),
            "test-token",
            &server.base_url(),
            true,
            false,
            &mut input,
            &mut stderr,
        )
        .await;

        match result {
            FinishLogin::Success(envelope) => {
                assert_failed(&envelope);
                let rendered = serde_json::to_string(&envelope).expect("serialize");
                assert!(!rendered.contains("secret backend detail"));
                assert!(!rendered.contains("test-token"));
            }
            FinishLogin::AgreementsRequired(_) => panic!("expected non-fatal success"),
        }
        status.assert_async().await;
        complete.assert_async().await;
    }

    #[test]
    fn write_rendered_routes_success_and_error_channels() {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let success = write_rendered("ok\n", 0, &mut stdout, &mut stderr);
        assert_eq!(success, std::process::ExitCode::SUCCESS);
        assert_eq!(stdout, b"ok\n");
        assert!(stderr.is_empty());

        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let failure = write_rendered("err\n", 1, &mut stdout, &mut stderr);
        assert_eq!(failure, std::process::ExitCode::from(1));
        assert!(stdout.is_empty());
        assert_eq!(stderr, b"err\n");
    }

    #[test]
    fn oauth_error_envelope_keeps_requested_format_and_exit_code() {
        let envelope = output::agreements_required_envelope();
        let rendered = output::render("json", &envelope).expect("render");
        assert!(rendered.contains("AGREEMENTS_REQUIRED"));

        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        assert_eq!(
            write_rendered(&rendered, 1, &mut stdout, &mut stderr),
            std::process::ExitCode::from(1)
        );
        assert!(stdout.is_empty());
        assert_eq!(stderr, rendered.as_bytes());
    }

    #[test]
    fn malformed_internal_json_falls_back_to_original_payload() {
        let login = CliRunOutput {
            exit_code: 2,
            rendered: "not-json".to_owned(),
        };
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        assert_eq!(
            write_rendered(&login.rendered, login.exit_code, &mut stdout, &mut stderr),
            std::process::ExitCode::from(2)
        );
        assert!(stdout.is_empty());
        assert_eq!(stderr, b"not-json");
    }
}
