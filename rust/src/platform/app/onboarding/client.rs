use reqwest::StatusCode;
use serde::de::DeserializeOwned;

use super::types::{
    ApiEnvelope, CliData, CliOnboardingResult, ErrorEnvelope, OnboardingStatus, StatusData,
};

/// Core's error code for a shopper that is not enabled for developer onboarding
/// (a `403` from `/onboarding/status` or `/onboarding/cli`).
const ONBOARDING_NOT_ENABLED_CODE: &str = "ONBOARDING_NOT_ENABLED";

/// An error body larger than this is not a core error envelope; do not parse it.
const MAX_ERROR_BODY_BYTES: usize = 8 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum OnboardingError {
    #[error("onboarding request could not be sent")]
    Request(#[source] reqwest::Error),
    #[error("onboarding service returned HTTP {0}")]
    Http(reqwest::StatusCode),
    #[error("developer onboarding is not enabled for this account")]
    NotEnabled,
    #[error("onboarding service returned an invalid response")]
    Decode(#[source] reqwest::Error),
    #[error("onboarding service rejected the request")]
    Unsuccessful,
}

pub struct OnboardingClient {
    base_url: String,
    http: reqwest::Client,
}

impl OnboardingClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_owned(),
            http: crate::http::api_client_builder()
                .build()
                .expect("onboarding HTTP client configuration is valid"),
        }
    }

    pub async fn status(&self, token: &str) -> Result<OnboardingStatus, OnboardingError> {
        let envelope: ApiEnvelope<StatusData> =
            self.post("/api/v1/onboarding/status", token).await?;
        if !envelope.success {
            return Err(OnboardingError::Unsuccessful);
        }

        Ok(OnboardingStatus {
            org_id: envelope.data.id,
            status: envelope.data.status,
        })
    }

    pub async fn complete(&self, token: &str) -> Result<CliOnboardingResult, OnboardingError> {
        let envelope: ApiEnvelope<CliData> = self.post("/api/v1/onboarding/cli", token).await?;
        if !envelope.success {
            return Err(OnboardingError::Unsuccessful);
        }

        Ok(CliOnboardingResult {
            organization_id: envelope.data.organization_id,
            status: envelope.data.status,
        })
    }

    async fn post<T: DeserializeOwned>(
        &self,
        path: &str,
        token: &str,
    ) -> Result<T, OnboardingError> {
        let response = self
            .http
            .post(format!("{}{path}", self.base_url))
            .bearer_auth(token)
            .json(&serde_json::json!({}))
            .send()
            .await
            .map_err(OnboardingError::Request)?;
        let status = response.status();
        if !status.is_success() {
            return Err(error_for_status(status, response).await);
        }

        response.json().await.map_err(OnboardingError::Decode)
    }
}

/// Classifies a non-success response. Only the `error.code` of a core error
/// envelope is read, and only to recognise `ONBOARDING_NOT_ENABLED`; the body is
/// never kept, logged or put in the error, since it may carry sensitive detail.
async fn error_for_status(status: StatusCode, response: reqwest::Response) -> OnboardingError {
    if status != StatusCode::FORBIDDEN {
        return OnboardingError::Http(status);
    }

    let code = read_body_capped(response, MAX_ERROR_BODY_BYTES)
        .await
        .and_then(|body| serde_json::from_slice::<ErrorEnvelope>(&body).ok())
        .map(|envelope| envelope.error.code);

    match code.as_deref() {
        Some(ONBOARDING_NOT_ENABLED_CODE) => OnboardingError::NotEnabled,
        _ => OnboardingError::Http(status),
    }
}

/// Reads a response body incrementally and gives up (`None`) as soon as it would
/// exceed `limit` bytes, or on a read error, so an oversized or endless error
/// body is never buffered whole. A declared `Content-Length` over the limit is
/// rejected without reading anything; the running total also covers bodies
/// with no length (chunked or streamed).
async fn read_body_capped(mut response: reqwest::Response, limit: usize) -> Option<Vec<u8>> {
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        return None;
    }

    let mut body = Vec::new();
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) => {
                if body.len() + chunk.len() > limit {
                    return None;
                }
                body.extend_from_slice(&chunk);
            }
            Ok(None) => return Some(body),
            Err(_) => return None,
        }
    }
}

#[cfg(test)]
mod tests {
    use httpmock::{Method::POST, MockServer};
    use serde_json::json;

    use reqwest::StatusCode;

    use super::{MAX_ERROR_BODY_BYTES, OnboardingClient, OnboardingError};
    use crate::platform::app::onboarding::{CliOnboardingResult, OnboardingStatus};

    #[tokio::test]
    async fn status_posts_bearer_and_parses_response() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/api/v1/onboarding/status")
                    .header("authorization", "Bearer test-token")
                    .header("user-agent", cli_engine::transport::default_user_agent())
                    .json_body(json!({}));
                then.status(200).json_body(json!({
                    "success": true,
                    "data": { "id": "org-1", "status": "PENDING" }
                }));
            })
            .await;

        let result = OnboardingClient::new(server.base_url())
            .status("test-token")
            .await
            .expect("status response");

        assert_eq!(
            result,
            OnboardingStatus {
                org_id: "org-1".to_owned(),
                status: "PENDING".to_owned(),
            }
        );
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn complete_posts_bearer_and_parses_response() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/api/v1/onboarding/cli")
                    .header("authorization", "Bearer test-token")
                    .header("user-agent", cli_engine::transport::default_user_agent())
                    .json_body(json!({}));
                then.status(200).json_body(json!({
                    "success": true,
                    "data": { "organizationId": "org-1", "status": "ACTIVE" }
                }));
            })
            .await;

        let result = OnboardingClient::new(server.base_url())
            .complete("test-token")
            .await
            .expect("completion response");

        assert_eq!(
            result,
            CliOnboardingResult {
                organization_id: "org-1".to_owned(),
                status: "ACTIVE".to_owned(),
            }
        );
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn errors_do_not_include_response_body_or_token() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(POST).path("/api/v1/onboarding/status");
                then.status(500).body("secret backend detail");
            })
            .await;

        let error = OnboardingClient::new(server.base_url())
            .status("test-token")
            .await
            .expect_err("status must fail");
        let message = error.to_string();

        assert!(message.contains("500"));
        assert!(!message.contains("secret backend detail"));
        assert!(!message.contains("test-token"));
    }

    #[tokio::test]
    async fn success_false_is_rejected() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(POST).path("/api/v1/onboarding/status");
                then.status(200).json_body(json!({
                    "success": false,
                    "data": { "id": "org-1", "status": "PENDING" }
                }));
            })
            .await;

        let error = OnboardingClient::new(server.base_url())
            .status("test-token")
            .await
            .expect_err("unsuccessful envelope");
        assert_eq!(error.to_string(), "onboarding service rejected the request");
    }

    fn not_enabled_body() -> serde_json::Value {
        json!({
            "success": false,
            "error": {
                "code": "ONBOARDING_NOT_ENABLED",
                "message": "Developer onboarding is not yet available for this account."
            }
        })
    }

    #[tokio::test]
    async fn status_maps_onboarding_not_enabled_forbidden() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(POST).path("/api/v1/onboarding/status");
                then.status(403).json_body(not_enabled_body());
            })
            .await;

        let error = OnboardingClient::new(server.base_url())
            .status("test-token")
            .await
            .expect_err("status must be rejected");

        assert!(matches!(error, OnboardingError::NotEnabled));
    }

    #[tokio::test]
    async fn complete_maps_onboarding_not_enabled_forbidden() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(POST).path("/api/v1/onboarding/cli");
                then.status(403).json_body(not_enabled_body());
            })
            .await;

        let error = OnboardingClient::new(server.base_url())
            .complete("test-token")
            .await
            .expect_err("complete must be rejected");

        assert!(matches!(error, OnboardingError::NotEnabled));
    }

    #[tokio::test]
    async fn other_forbidden_codes_stay_generic_http_errors() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(POST).path("/api/v1/onboarding/status");
                then.status(403).json_body(json!({
                    "success": false,
                    "error": { "code": "FORBIDDEN", "message": "secret backend detail" }
                }));
            })
            .await;

        let error = OnboardingClient::new(server.base_url())
            .status("test-token")
            .await
            .expect_err("status must fail");

        assert!(matches!(
            error,
            OnboardingError::Http(StatusCode::FORBIDDEN)
        ));
        assert!(!error.to_string().contains("secret backend detail"));
    }

    #[tokio::test]
    async fn forbidden_with_an_unparseable_body_stays_a_generic_http_error() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(POST).path("/api/v1/onboarding/status");
                then.status(403).body("<html>ONBOARDING_NOT_ENABLED</html>");
            })
            .await;

        let error = OnboardingClient::new(server.base_url())
            .status("test-token")
            .await
            .expect_err("status must fail");

        assert!(matches!(
            error,
            OnboardingError::Http(StatusCode::FORBIDDEN)
        ));
    }

    #[tokio::test]
    async fn an_oversized_forbidden_body_is_not_parsed() {
        let server = MockServer::start_async().await;
        let padding = "x".repeat(MAX_ERROR_BODY_BYTES + 1);
        server
            .mock_async(|when, then| {
                when.method(POST).path("/api/v1/onboarding/status");
                then.status(403).json_body(json!({
                    "success": false,
                    "error": { "code": "ONBOARDING_NOT_ENABLED", "message": padding }
                }));
            })
            .await;

        let error = OnboardingClient::new(server.base_url())
            .status("test-token")
            .await
            .expect_err("status must fail");

        assert!(matches!(
            error,
            OnboardingError::Http(StatusCode::FORBIDDEN)
        ));
    }

    #[tokio::test]
    async fn the_not_enabled_code_only_counts_on_a_forbidden_status() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(POST).path("/api/v1/onboarding/status");
                then.status(500).json_body(not_enabled_body());
            })
            .await;

        let error = OnboardingClient::new(server.base_url())
            .status("test-token")
            .await
            .expect_err("status must fail");

        assert!(matches!(
            error,
            OnboardingError::Http(StatusCode::INTERNAL_SERVER_ERROR)
        ));
    }

    #[tokio::test]
    async fn not_enabled_errors_do_not_include_response_body_or_token() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(POST).path("/api/v1/onboarding/status");
                then.status(403).json_body(json!({
                    "success": false,
                    "error": {
                        "code": "ONBOARDING_NOT_ENABLED",
                        "message": "secret backend detail"
                    }
                }));
            })
            .await;

        let error = OnboardingClient::new(server.base_url())
            .status("test-token")
            .await
            .expect_err("status must fail");
        let message = error.to_string();

        assert!(!message.contains("secret backend detail"));
        assert!(!message.contains("test-token"));
    }

    #[tokio::test]
    async fn a_forbidden_body_exactly_at_the_limit_is_still_parsed() {
        let envelope = |message: &str| {
            json!({
                "success": false,
                "error": { "code": "ONBOARDING_NOT_ENABLED", "message": message }
            })
            .to_string()
        };
        let padding = "x".repeat(MAX_ERROR_BODY_BYTES - envelope("").len());
        let body = envelope(&padding);
        assert_eq!(body.len(), MAX_ERROR_BODY_BYTES);

        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(POST).path("/api/v1/onboarding/status");
                then.status(403)
                    .header("content-type", "application/json")
                    .body(body);
            })
            .await;

        let error = OnboardingClient::new(server.base_url())
            .status("test-token")
            .await
            .expect_err("status must fail");

        assert!(matches!(error, OnboardingError::NotEnabled));
    }
}
