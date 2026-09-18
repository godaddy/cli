use email_client::types;

const USER_AGENT: &str = concat!("godaddy-cli/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("HTTP error {status}: {body}")]
    Http { status: u16, body: String },
    #[error("network error: {0}")]
    Network(String),
    #[error("request error: {0}")]
    Request(String),
    #[error("failed to decode Email API response: {0}")]
    Response(#[from] serde_json::Error),
    #[error("failed to construct Email API client: {0}")]
    Build(#[from] email_client::BuildError),
}

impl From<ClientError> for crate::error::GddyError {
    fn from(value: ClientError) -> Self {
        match value {
            ClientError::Http { status, body } => Self::from_http(status, body, "email"),
            ClientError::Network(error) | ClientError::Request(error) => {
                Self::network(error).with_system("email")
            }
            ClientError::Response(error) => {
                Self::unexpected(format!("failed to decode Email API response: {error}"))
                    .with_system("email")
            }
            ClientError::Build(error) => {
                Self::config(format!("failed to construct Email API client: {error}"))
                    .with_system("email")
            }
        }
    }
}

pub struct EmailClient {
    client: email_client::Client,
}

impl EmailClient {
    pub fn new(base_url: impl AsRef<str>, token: impl AsRef<str>) -> Result<Self, ClientError> {
        let client = email_client::client_with_auth(
            base_url.as_ref(),
            &format!("Bearer {}", token.as_ref()),
            USER_AGENT,
            &uuid::Uuid::new_v4().to_string(),
        )
        .map_err(ClientError::Build)?;
        Ok(Self { client })
    }

    pub async fn list_mailboxes(
        &self,
        status: Option<&str>,
        page: u32,
        page_size: u32,
        field: Option<&str>,
    ) -> Result<types::MailboxList, ClientError> {
        let mut request = self
            .client
            .list_mailboxes()
            .page(u64::from(page))
            .page_size(u64::from(page_size));
        if let Some(status) = status {
            request = request.status(status.to_owned());
        }
        if let Some(field) = field {
            request = request.field(field.to_owned());
        }
        response(request.send().await).await
    }

    pub async fn get_mailbox(&self, mailbox_id: &str) -> Result<types::Mailbox, ClientError> {
        response(
            self.client
                .get_mailbox()
                .mailbox_id(mailbox_id)
                .send()
                .await,
        )
        .await
    }

    pub async fn create_mailbox(
        &self,
        body: types::CreateMailboxBody,
    ) -> Result<types::CreateMailboxResponse, ClientError> {
        let idempotency_key = uuid::Uuid::new_v4().to_string();
        response(
            self.client
                .create_mailbox()
                .idempotency_key(idempotency_key)
                .body(body)
                .send()
                .await,
        )
        .await
    }

    pub async fn check_eligibility(
        &self,
        email: &str,
    ) -> Result<types::EligibilityResult, ClientError> {
        response(
            self.client
                .check_mailbox_eligibility()
                .email(email)
                .send()
                .await,
        )
        .await
    }
}

async fn response<T>(
    result: Result<progenitor_client::ResponseValue<T>, email_client::Error<types::Error>>,
) -> Result<T, ClientError> {
    match result {
        Ok(response) => Ok(response.into_inner()),
        Err(email_client::Error::ErrorResponse(response)) => {
            let status = response.status().as_u16();
            let body = serde_json::to_string(response.as_ref()).unwrap_or_default();
            Err(ClientError::Http { status, body })
        }
        Err(email_client::Error::UnexpectedResponse(response)) => {
            let status = response.status().as_u16();
            let body = response.text().await.unwrap_or_default();
            Err(ClientError::Http { status, body })
        }
        Err(email_client::Error::CommunicationError(error)) => {
            Err(ClientError::Network(error.to_string()))
        }
        Err(email_client::Error::InvalidResponsePayload(_, error)) => {
            Err(ClientError::Response(error))
        }
        Err(error) => Err(ClientError::Request(error.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use httpmock::prelude::*;
    use serde_json::json;

    use super::*;

    fn client(base_url: &str) -> EmailClient {
        EmailClient::new(base_url, "test-token").expect("client should build")
    }

    fn new_mailbox_body(email: &str) -> types::CreateMailboxBody {
        types::CreateMailboxBody {
            account_id: None,
            consents: vec![],
            created_at: None,
            display_name: None,
            email_address: email.to_owned(),
            first_name: None,
            last_name: None,
            links: vec![],
            mailbox_id: None,
            mailbox_type: None,
            status: None,
            updated_at: None,
        }
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
                then.status(200).json_body(json!({ "items": [] }));
            })
            .await;

        let list = client(&server.base_url())
            .list_mailboxes(Some("ACTIVE"), 1, 100, None)
            .await
            .expect("list mailboxes");

        mock.assert_async().await;
        assert!(list.items.is_empty());
    }

    #[tokio::test]
    async fn get_mailbox_sends_bearer_auth_and_parses_typed_status() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/v1/email/mailboxes/mbx-456")
                    .header("authorization", "Bearer test-token");
                then.status(200).json_body(json!({
                    "mailboxId": "mbx-456",
                    "emailAddress": "someone@example.com",
                    "status": "COMPLETED"
                }));
            })
            .await;

        let mailbox = client(&server.base_url())
            .get_mailbox("mbx-456")
            .await
            .expect("get mailbox");

        mock.assert_async().await;
        assert_eq!(
            mailbox.mailbox_id.map(|id| id.to_string()),
            Some("mbx-456".to_owned())
        );
        assert_eq!(
            mailbox.status.map(|status| status.to_string()),
            Some("COMPLETED".to_owned())
        );
    }

    #[tokio::test]
    async fn create_mailbox_posts_json_body() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v1/email/mailboxes")
                    .header("authorization", "Bearer test-token")
                    .json_body(json!({ "emailAddress": "someone@example.com" }));
                then.status(202).json_body(json!({
                    "mailboxId": "mbx-456",
                    "emailAddress": "someone@example.com",
                    "status": "EXECUTING"
                }));
            })
            .await;

        let created = client(&server.base_url())
            .create_mailbox(new_mailbox_body("someone@example.com"))
            .await
            .expect("create mailbox");

        mock.assert_async().await;
        assert_eq!(
            created.status.map(|status| status.to_string()),
            Some("EXECUTING".to_owned())
        );
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

        let result = client(&server.base_url())
            .check_eligibility("someone@example.com")
            .await
            .expect("check eligibility");

        mock.assert_async().await;
        assert!(result.is_eligible);
    }

    #[tokio::test]
    async fn create_mailbox_sends_an_idempotency_key_header() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v1/email/mailboxes")
                    .header_exists("idempotency-key");
                then.status(202).json_body(json!({
                    "mailboxId": "mbx-456",
                    "status": "EXECUTING"
                }));
            })
            .await;

        client(&server.base_url())
            .create_mailbox(new_mailbox_body("someone@example.com"))
            .await
            .expect("create mailbox");

        mock.assert_async().await;
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
            .create_mailbox(new_mailbox_body("someone@example.com"))
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
