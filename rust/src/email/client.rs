use reqwest::{Client, Method};
use serde_json::{Value, json};

use crate::application::client::make_http_client;

const BASE_PATH: &str = "/v1/email";

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("HTTP error {status}: {body}")]
    Http { status: u16, body: String },
    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),
}

impl From<ClientError> for crate::error::GddyError {
    fn from(value: ClientError) -> Self {
        match value {
            ClientError::Http { status, body } => Self::from_http(status, body, "email"),
            ClientError::Network(e) => {
                Self::network(format!("network error: {e}")).with_system("email")
            }
        }
    }
}

pub struct EmailClient {
    client: Client,
    base_url: String,
    token: String,
}

impl EmailClient {
    pub fn new(base_url: impl Into<String>, token: impl Into<String>) -> Self {
        Self {
            client: make_http_client(),
            base_url: base_url.into(),
            token: token.into(),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{BASE_PATH}{path}", self.base_url)
    }

    fn new_request_id() -> String {
        uuid::Uuid::new_v4().to_string()
    }

    async fn send_json(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, String)],
        extra_headers: &[(&str, String)],
        body: Option<Value>,
    ) -> Result<Value, ClientError> {
        let mut req = self
            .client
            .request(method, self.url(path))
            .bearer_auth(&self.token)
            .header("x-request-id", Self::new_request_id());
        for (key, value) in extra_headers {
            req = req.header(*key, value);
        }
        for (key, value) in query {
            req = req.query(&[(key, value)]);
        }
        if let Some(body) = body {
            req = req.json(&body);
        }
        let request = req.build()?;
        cli_engine::transport::debug_log_reqwest_request(&request);
        let resp = self.client.execute(request).await?;

        let status = resp.status();
        let headers = resp.headers().clone();
        let bytes = resp.bytes().await?;
        cli_engine::transport::debug_log_reqwest_response(status, &headers, &bytes);

        let status = status.as_u16();
        if status == 204 {
            return Ok(json!(null));
        }
        if !(200..300).contains(&status) {
            return Err(ClientError::Http {
                status,
                body: String::from_utf8_lossy(&bytes).into_owned(),
            });
        }
        if bytes.is_empty() {
            return Ok(json!(null));
        }
        serde_json::from_slice(&bytes).map_err(|e| ClientError::Http {
            status,
            body: format!(
                "invalid JSON response: {e} (body: {})",
                String::from_utf8_lossy(&bytes)
            ),
        })
    }

    pub async fn list_mailboxes(&self, query: &[(&str, String)]) -> Result<Value, ClientError> {
        self.send_json(Method::GET, "/mailboxes", query, &[], None)
            .await
    }

    pub async fn get_mailbox(&self, mailbox_id: &str) -> Result<Value, ClientError> {
        self.send_json(
            Method::GET,
            &format!("/mailboxes/{mailbox_id}"),
            &[],
            &[],
            None,
        )
        .await
    }

    pub async fn create_mailbox(&self, body: Value) -> Result<Value, ClientError> {
        let idempotency_key = uuid::Uuid::new_v4().to_string();
        self.send_json(
            Method::POST,
            "/mailboxes",
            &[],
            &[("idempotency-key", idempotency_key)],
            Some(body),
        )
        .await
    }

    pub async fn check_eligibility(&self, email: &str) -> Result<Value, ClientError> {
        self.send_json(
            Method::GET,
            "/check-mailbox-eligibility",
            &[("email", email.to_owned())],
            &[],
            None,
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use httpmock::prelude::*;
    use serde_json::json;

    use super::*;

    fn client(base_url: &str) -> EmailClient {
        EmailClient::new(base_url, "test-token")
    }

    #[tokio::test]
    async fn list_mailboxes_sends_bearer_auth_and_query_params() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/v1/email/mailboxes")
                    .header("authorization", "Bearer test-token")
                    .query_param("status", "ACTIVE")
                    .query_param("page", "1");
                then.status(200).json_body(json!({ "mailboxes": [] }));
            })
            .await;

        let body = client(&server.base_url())
            .list_mailboxes(&[("status", "ACTIVE".to_owned()), ("page", "1".to_owned())])
            .await
            .expect("list mailboxes");

        mock.assert_async().await;
        assert_eq!(body["mailboxes"], json!([]));
    }

    #[tokio::test]
    async fn get_mailbox_sends_bearer_auth() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/v1/email/mailboxes/mbx-456")
                    .header("authorization", "Bearer test-token");
                then.status(200)
                    .json_body(json!({ "mailboxId": "mbx-456", "status": "CONFIRMED" }));
            })
            .await;

        let body = client(&server.base_url())
            .get_mailbox("mbx-456")
            .await
            .expect("get mailbox");

        mock.assert_async().await;
        assert_eq!(body["mailboxId"], "mbx-456");
    }

    #[tokio::test]
    async fn create_mailbox_posts_json_body() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v1/email/mailboxes")
                    .header("authorization", "Bearer test-token")
                    .header_exists("idempotency-key")
                    .json_body(json!({ "emailAddress": "someone@example.com" }));
                then.status(202)
                    .json_body(json!({ "mailboxId": "mbx-456", "status": "EXECUTING" }));
            })
            .await;

        let body = client(&server.base_url())
            .create_mailbox(json!({ "emailAddress": "someone@example.com" }))
            .await
            .expect("create mailbox");

        mock.assert_async().await;
        assert_eq!(body["status"], "EXECUTING");
    }

    #[tokio::test]
    async fn check_eligibility_sends_email_query_param() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/v1/email/check-mailbox-eligibility")
                    .header("authorization", "Bearer test-token")
                    .query_param("email", "someone@example.com");
                then.status(200).json_body(json!({ "isEligible": true }));
            })
            .await;

        let body = client(&server.base_url())
            .check_eligibility("someone@example.com")
            .await
            .expect("check eligibility");

        mock.assert_async().await;
        assert_eq!(body["isEligible"], true);
    }

    #[tokio::test]
    async fn create_mailbox_surfaces_business_rule_error_body() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(POST).path("/v1/email/mailboxes");
                then.status(422).json_body(json!({
                    "name": "UnprocessableEntity",
                    "message": "missing required agreements",
                    "correlationId": "corr-1",
                    "details": [{ "issue": "MISSING_AGREEMENT", "description": "EMAIL_TOS not accepted" }]
                }));
            })
            .await;

        let err = client(&server.base_url())
            .create_mailbox(json!({ "emailAddress": "someone@example.com" }))
            .await
            .expect_err("business-rule failure should surface as an error");

        mock.assert_async().await;
        let ClientError::Http { status, body } = err else {
            unreachable!("expected an HTTP error");
        };
        assert_eq!(status, 422);
        assert!(body.contains("MISSING_AGREEMENT"), "{body}");
    }
}
