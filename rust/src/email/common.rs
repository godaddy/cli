//! Shared helpers for the `email` command group: the authenticated Email
//! (panel-v3) API client and API-error rendering.

use cli_engine::{CliCoreError, CommandContext, Result};
use serde::Deserialize;

use crate::email::client::{ClientError, EmailClient};
use crate::error::GddyError;

pub(crate) async fn make_client(ctx: &CommandContext, scopes: &[&str]) -> Result<EmailClient> {
    let required: Vec<String> = scopes.iter().map(|s| (*s).to_owned()).collect();
    let token = ctx.credential_with_scopes(&required).await?.token;
    let base_url = crate::environments::resolve(&ctx.middleware.env)?.api_url;
    Ok(EmailClient::new(base_url, token))
}

/// Maps a [`ClientError`] to a [`CliCoreError`], rendering the panel API's
/// `{message, details: [{issue, description}]}` error-body shape into a
/// human-readable message. Distinct from `domain::common::format_api_error`:
/// the panel API's error envelope doesn't carry the `fields`/402-payment
/// shape that helper is built around, so this is its own (simpler) renderer.
pub(crate) fn client_err(e: ClientError) -> CliCoreError {
    match e {
        ClientError::Http { status, body } => {
            GddyError::from_http(status, format_api_error_body(&body), "email").into_cli_error()
        }
        ClientError::Network(_) => GddyError::from(e).into_cli_error(),
    }
}

/// Like [`client_err`], but overrides the fix hint. `create` uses this to
/// point business-rule failures (missing agreements, no eligible account) at
/// `email check-eligibility` instead of the generic "check request body" hint.
pub(crate) fn client_err_with_fix(e: ClientError, fix: impl Into<String>) -> CliCoreError {
    match e {
        ClientError::Http { status, body } => {
            GddyError::from_http(status, format_api_error_body(&body), "email")
                .with_fix(fix)
                .into_cli_error()
        }
        ClientError::Network(_) => GddyError::from(e).into_cli_error(),
    }
}

#[derive(Debug, Deserialize)]
struct ApiErrorBody {
    message: Option<String>,
    #[serde(default)]
    details: Vec<ApiErrorDetail>,
}

#[derive(Debug, Deserialize)]
struct ApiErrorDetail {
    issue: Option<String>,
    description: Option<String>,
}

fn format_api_error_body(body: &str) -> String {
    let Ok(parsed) = serde_json::from_str::<ApiErrorBody>(body) else {
        return body.to_owned();
    };
    let message = parsed.message.unwrap_or_else(|| body.to_owned());
    let details: Vec<String> = parsed
        .details
        .iter()
        .filter_map(|d| d.description.clone().or_else(|| d.issue.clone()))
        .filter(|s| !s.is_empty())
        .collect();
    if details.is_empty() {
        message
    } else {
        format!("{message} ({})", details.join("; "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_api_error_body_renders_detail_descriptions() {
        let body = r#"{
            "name": "UnprocessableEntity",
            "message": "missing required agreements",
            "correlationId": "corr-1",
            "details": [{ "issue": "MISSING_AGREEMENT", "description": "EMAIL_TOS not accepted" }]
        }"#;
        let rendered = format_api_error_body(body);
        assert_eq!(
            rendered,
            "missing required agreements (EMAIL_TOS not accepted)"
        );
    }

    #[test]
    fn format_api_error_body_falls_back_to_issue_without_description() {
        let body = r#"{"message": "bad request", "details": [{"issue": "NO_ELIGIBLE_ACCOUNT"}]}"#;
        assert_eq!(
            format_api_error_body(body),
            "bad request (NO_ELIGIBLE_ACCOUNT)"
        );
    }

    #[test]
    fn format_api_error_body_passes_through_unparseable_bodies() {
        assert_eq!(format_api_error_body("not json"), "not json");
    }

    #[test]
    fn client_err_with_fix_overrides_the_default_fix() {
        let err = client_err_with_fix(
            ClientError::Http {
                status: 422,
                body: "{\"message\": \"missing required agreements\"}".to_owned(),
            },
            "Run: gddy email check-eligibility --email someone@example.com",
        );
        let envelope = cli_engine::build_error_envelope(&err, "email");
        assert!(
            envelope
                .fix
                .as_deref()
                .is_some_and(|f| f.contains("check-eligibility")),
            "{envelope:?}"
        );
    }

    #[test]
    fn client_err_maps_http_status_to_email_system() {
        let err = client_err(ClientError::Http {
            status: 404,
            body: "{\"message\": \"mailbox not found\"}".to_owned(),
        });
        let envelope = cli_engine::build_error_envelope(&err, "email");
        assert_eq!(
            envelope.error.as_ref().map(|e| e.message.as_str()),
            Some("HTTP error 404: mailbox not found")
        );
        assert!(
            envelope
                .fix
                .as_deref()
                .is_some_and(|f| f.contains("email list")),
            "{envelope:?}"
        );
    }
}
