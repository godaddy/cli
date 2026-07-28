use serde::de::DeserializeOwned;

use super::types::{ApiEnvelope, CliData, CliOnboardingResult, OnboardingStatus, StatusData};

const USER_AGENT: &str = concat!("godaddy-cli/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, thiserror::Error)]
pub enum OnboardingError {
    #[error("onboarding request could not be sent")]
    Request(#[source] reqwest::Error),
    #[error("onboarding service returned HTTP {0}")]
    Http(reqwest::StatusCode),
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
            http: reqwest::Client::builder()
                .user_agent(USER_AGENT)
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
        if !response.status().is_success() {
            return Err(OnboardingError::Http(response.status()));
        }

        response.json().await.map_err(OnboardingError::Decode)
    }
}

#[cfg(test)]
mod tests {
    use httpmock::{Method::POST, MockServer};
    use serde_json::json;

    use super::{OnboardingClient, USER_AGENT};
    use crate::onboarding::{CliOnboardingResult, OnboardingStatus};

    #[tokio::test]
    async fn status_posts_bearer_and_parses_response() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/api/v1/onboarding/status")
                    .header("authorization", "Bearer test-token")
                    .header("user-agent", USER_AGENT)
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
                    .header("user-agent", USER_AGENT)
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
}
