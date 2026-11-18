//! HTTP client for the Domain Connect Templates API.
//!
//! Spec: gdcorp-platform/domains.domain-connect-specification v2.
//! Both operations are controllers under `/v2/domains/` and return a
//! `TemplateActionResult` on `200`; submit also returns it on `409` (blocked)
//! and `422` (invalid). Every other status carries the platform `Error`.
//!
//! The template is sent exactly as read from the user's file. It is never
//! decoded into [`super::Template`] on this path: the upstream schema allows
//! `"ttl": "%ttl%"` and fields the struct does not model, and the API forwards
//! the bytes unchanged to GoDaddy DNS and the pull request.

use reqwest::StatusCode;
use serde::Deserialize;
use serde_json::Value;

use crate::error::GddyError;

const VALIDATE_PATH: &str = "/v2/domains/validate-domain-connect-template";
const SUBMIT_PATH: &str = "/v2/domains/submit-domain-connect-template";
const SYSTEM: &str = "domain-connect";

/// `TemplateActionResult` from the spec. `outcome` decides the verdict;
/// `issues` explain it; `pr` is present after a successful submit.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ActionResult {
    pub(crate) action: String,
    pub(crate) outcome: String,
    #[serde(default)]
    pub(crate) operation: Option<String>,
    #[serde(default)]
    pub(crate) issues: Vec<Issue>,
    #[serde(default)]
    pub(crate) pr: Option<PullRequest>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Issue {
    pub(crate) code: String,
    pub(crate) level: String,
    pub(crate) message: String,
    #[serde(default)]
    pub(crate) field: Option<String>,
    #[serde(default)]
    pub(crate) fix: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PullRequest {
    #[serde(default)]
    pub(crate) number: Option<u64>,
    #[serde(default)]
    pub(crate) url: Option<String>,
    #[serde(default)]
    pub(crate) state: Option<String>,
}

/// Platform `Error` (common-types `error.yaml`).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiError {
    name: String,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    details: Vec<ApiErrorDetail>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiErrorDetail {
    #[serde(default)]
    field: Option<String>,
    issue: String,
    #[serde(default)]
    description: Option<String>,
}

/// Outcome values from the spec's `TemplateActionOutcome` enum.
pub(crate) mod outcome {
    pub(crate) const READY: &str = "READY";
    pub(crate) const INVALID: &str = "INVALID";
    pub(crate) const BLOCKED: &str = "BLOCKED";
    pub(crate) const SUBMITTED: &str = "SUBMITTED";
}

pub(crate) struct TemplatesClient {
    http: reqwest::Client,
    base_url: String,
    token: String,
}

impl TemplatesClient {
    pub(crate) fn new(base_url: impl Into<String>, token: impl Into<String>) -> Self {
        Self {
            http: crate::http::make_http_client(),
            base_url: base_url.into().trim_end_matches('/').to_owned(),
            token: token.into(),
        }
    }

    /// `POST /v2/domains/validate-domain-connect-template`. Always `200` for a
    /// processable template; the verdict is in `outcome`.
    pub(crate) async fn validate(&self, template: &Value) -> cli_engine::Result<ActionResult> {
        let (status, result) = self.post(VALIDATE_PATH, template).await?;
        match (status, result) {
            (StatusCode::OK, Body::Result(result)) => Ok(result),
            (status, body) => Err(unexpected(status, body)),
        }
    }

    /// `POST /v2/domains/submit-domain-connect-template`. `200`, `409` and
    /// `422` all carry an `ActionResult`; the caller reads `outcome`.
    pub(crate) async fn submit(&self, template: &Value) -> cli_engine::Result<ActionResult> {
        let (status, result) = self.post(SUBMIT_PATH, template).await?;
        match (status, result) {
            (
                StatusCode::OK | StatusCode::CONFLICT | StatusCode::UNPROCESSABLE_ENTITY,
                Body::Result(result),
            ) => Ok(result),
            (status, body) => Err(unexpected(status, body)),
        }
    }

    async fn post(&self, path: &str, template: &Value) -> cli_engine::Result<(StatusCode, Body)> {
        let request = self
            .http
            .post(format!("{}{path}", self.base_url))
            .bearer_auth(&self.token)
            .header("x-request-id", uuid::Uuid::new_v4().to_string())
            .json(&serde_json::json!({ "template": template }))
            .build()
            .map_err(|e| GddyError::validation(e.to_string()).into_cli_error())?;
        cli_engine::transport::debug_log_reqwest_request(&request);
        let response = self
            .http
            .execute(request)
            .await
            .map_err(|e| GddyError::network(e.to_string()).into_cli_error())?;
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = response
            .bytes()
            .await
            .map_err(|e| GddyError::network(e.to_string()).into_cli_error())?;
        cli_engine::transport::debug_log_reqwest_response(status, &headers, &bytes);

        if let Ok(result) = serde_json::from_slice::<ActionResult>(&bytes) {
            return Ok((status, Body::Result(result)));
        }
        if let Ok(error) = serde_json::from_slice::<ApiError>(&bytes) {
            return Ok((status, Body::Error(error)));
        }
        Ok((
            status,
            Body::Other(String::from_utf8_lossy(&bytes).into_owned()),
        ))
    }
}

enum Body {
    Result(ActionResult),
    Error(ApiError),
    Other(String),
}

fn unexpected(status: StatusCode, body: Body) -> cli_engine::CliCoreError {
    let code = status.as_u16();
    match body {
        Body::Error(error) => {
            let mut message = error.name.clone();
            if let Some(text) = &error.message {
                message.push_str(": ");
                message.push_str(text);
            }
            for detail in &error.details {
                message.push_str("\n  ");
                if let Some(field) = &detail.field {
                    message.push_str(field);
                    message.push(' ');
                }
                message.push_str(&detail.issue);
                if let Some(description) = &detail.description {
                    message.push_str(": ");
                    message.push_str(description);
                }
            }
            match code {
                401 => GddyError::auth(message).with_system(SYSTEM),
                403 => GddyError::from_http(code, message, SYSTEM).with_fix(
                    "Run `gddy auth login` to grant the domains.domain-connect-template scopes.",
                ),
                _ => GddyError::from_http(code, message, SYSTEM),
            }
        }
        Body::Result(result) => GddyError::unexpected(format!(
            "HTTP {code} with outcome {} was not expected from this operation",
            result.outcome
        ))
        .with_system(SYSTEM),
        Body::Other(text) => GddyError::from_http(code, text, SYSTEM),
    }
    .into_cli_error()
}

#[cfg(test)]
mod tests {
    use httpmock::prelude::*;
    use serde_json::json;

    use super::*;

    fn template() -> Value {
        json!({
            "providerId": "acme.com",
            "providerName": "Acme",
            "serviceId": "svc",
            "serviceName": "Svc",
            "version": 1,
            "warnPhishing": true,
            "records": [{ "type": "A", "host": "@", "pointsTo": "203.0.113.10", "ttl": "%ttl%" }]
        })
    }

    #[tokio::test]
    async fn validate_posts_the_template_verbatim_and_reads_outcome() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v2/domains/validate-domain-connect-template")
                    .header("authorization", "Bearer test-token")
                    .header_exists("x-request-id")
                    .json_body(json!({ "template": template() }));
                then.status(200).json_body(json!({
                    "action": "VALIDATE", "outcome": "READY", "operation": "ADD", "issues": []
                }));
            })
            .await;

        let client = TemplatesClient::new(server.base_url(), "test-token");
        let result = client.validate(&template()).await.expect("ok");

        mock.assert_async().await;
        assert_eq!(result.outcome, outcome::READY);
        assert_eq!(result.operation.as_deref(), Some("ADD"));
        assert!(result.issues.is_empty());
    }

    #[tokio::test]
    async fn submit_treats_409_and_422_as_results_not_errors() {
        let server = MockServer::start_async().await;
        for (status, outcome, code) in [
            (409, "BLOCKED", "PROVIDER_OWNED_BY_OTHER"),
            (422, "INVALID", "SCHEMA"),
        ] {
            let server_mock = server
                .mock_async(|when, then| {
                    when.method(POST)
                        .path("/v2/domains/submit-domain-connect-template");
                    then.status(status).json_body(json!({
                        "action": "SUBMIT", "outcome": outcome, "operation": "ADD",
                        "issues": [{ "code": code, "level": "ERROR", "message": "m", "field": "providerId" }]
                    }));
                })
                .await;
            let client = TemplatesClient::new(server.base_url(), "test-token");
            let result = client.submit(&template()).await.expect("result");
            assert_eq!(result.outcome, outcome);
            assert_eq!(result.issues[0].code, code);
            server_mock.delete_async().await;
        }
    }

    #[tokio::test]
    async fn submit_returns_pull_request_on_200() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v2/domains/submit-domain-connect-template");
                then.status(200).json_body(json!({
                    "action": "SUBMIT", "outcome": "SUBMITTED", "operation": "ADD", "issues": [],
                    "pr": { "number": 1234, "url": "https://github.com/Domain-Connect/Templates/pull/1234", "state": "FAST_TRACK" }
                }));
            })
            .await;

        let client = TemplatesClient::new(server.base_url(), "test-token");
        let result = client.submit(&template()).await.expect("ok");
        let pr = result.pr.expect("pr present");
        assert_eq!(pr.number, Some(1234));
        assert_eq!(pr.state.as_deref(), Some("FAST_TRACK"));
    }

    #[tokio::test]
    async fn platform_error_is_surfaced_with_name_and_details() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v2/domains/validate-domain-connect-template");
                then.status(403).json_body(json!({
                    "name": "FORBIDDEN",
                    "correlationId": "550e8400-e29b-41d4-a716-446655440000",
                    "message": "Authenticated identity is not authorized to perform this operation."
                }));
            })
            .await;

        let client = TemplatesClient::new(server.base_url(), "test-token");
        let err = client
            .validate(&template())
            .await
            .expect_err("403 is an error");
        let text = err.to_string();
        assert!(text.contains("FORBIDDEN"), "{text}");
        assert!(text.contains("not authorized"), "{text}");
    }

    #[tokio::test]
    async fn validate_rejects_an_unexpected_status() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v2/domains/validate-domain-connect-template");
                then.status(409).json_body(json!({
                    "action": "VALIDATE", "outcome": "BLOCKED", "issues": []
                }));
            })
            .await;

        let client = TemplatesClient::new(server.base_url(), "test-token");
        let err = client
            .validate(&template())
            .await
            .expect_err("409 is not a validate status");
        assert!(err.to_string().contains("409"), "{err}");
    }
}
