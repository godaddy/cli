use std::time::Duration;

use reqwest::{Client, Method};
use serde_json::{Value, json};

use crate::application::client::make_http_client;

const BASE_PATH: &str = "/v1/shopping";

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("HTTP error {status}: {body}")]
    Http {
        status: u16,
        body: String,
        retry_after: Option<Duration>,
    },
    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),
}

impl ClientError {
    pub fn is_retryable_order_read(&self) -> bool {
        matches!(self, Self::Network(_))
            || matches!(
                self,
                Self::Http {
                    status: 404 | 429 | 500..=599,
                    ..
                }
            )
    }

    pub fn retry_after(&self) -> Option<Duration> {
        match self {
            Self::Http { retry_after, .. } => *retry_after,
            Self::Network(_) => None,
        }
    }
}

impl From<ClientError> for crate::error::GddyError {
    fn from(value: ClientError) -> Self {
        match value {
            ClientError::Http { status, body, .. } => Self::from_http(status, body, "shopping"),
            ClientError::Network(error) => {
                Self::network(format!("network error: {error}")).with_system("shopping")
            }
        }
    }
}

pub struct ShoppingClient {
    client: Client,
    base_url: String,
    token: String,
}

impl ShoppingClient {
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

    async fn send_json(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<Value, ClientError> {
        let mut request = self
            .client
            .request(method, self.url(path))
            .bearer_auth(&self.token)
            .header("x-request-id", uuid::Uuid::new_v4().to_string());
        if let Some(body) = body {
            request = request.json(&body);
        }
        let request = request.build()?;
        let response = self.client.execute(request).await?;
        let status = response.status();
        let headers = response.headers().clone();
        let retry_after = headers
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok())
            .map(Duration::from_secs);
        let bytes = response.bytes().await?;

        let status = status.as_u16();
        if status == 204 || bytes.is_empty() {
            return if (200..300).contains(&status) {
                Ok(json!(null))
            } else {
                Err(ClientError::Http {
                    status,
                    body: String::new(),
                    retry_after,
                })
            };
        }
        if !(200..300).contains(&status) {
            return Err(ClientError::Http {
                status,
                body: String::from_utf8_lossy(&bytes).into_owned(),
                retry_after,
            });
        }
        serde_json::from_slice(&bytes).map_err(|error| ClientError::Http {
            status,
            body: format!(
                "invalid JSON response: {error} (body: {})",
                String::from_utf8_lossy(&bytes)
            ),
            retry_after: None,
        })
    }

    pub async fn catalog_search(&self, body: Value) -> Result<Value, ClientError> {
        self.send_json(Method::POST, "/catalog/search", Some(body))
            .await
    }

    pub async fn catalog_lookup(&self, body: Value) -> Result<Value, ClientError> {
        self.send_json(Method::POST, "/catalog/lookup", Some(body))
            .await
    }

    pub async fn catalog_product(&self, body: Value) -> Result<Value, ClientError> {
        self.send_json(Method::POST, "/catalog/product", Some(body))
            .await
    }

    pub async fn create_checkout(&self, body: Value) -> Result<Value, ClientError> {
        self.send_json(Method::POST, "/checkout-sessions", Some(body))
            .await
    }

    pub async fn get_checkout(&self, id: &str) -> Result<Value, ClientError> {
        self.send_json(Method::GET, &format!("/checkout-sessions/{id}"), None)
            .await
    }

    pub async fn update_checkout(&self, id: &str, body: Value) -> Result<Value, ClientError> {
        self.send_json(Method::PUT, &format!("/checkout-sessions/{id}"), Some(body))
            .await
    }

    pub async fn complete_checkout(&self, id: &str, body: Value) -> Result<Value, ClientError> {
        self.send_json(
            Method::POST,
            &format!("/checkout-sessions/{id}/complete"),
            Some(body),
        )
        .await
    }

    pub async fn get_order(&self, id: &str) -> Result<Value, ClientError> {
        self.send_json(Method::GET, &format!("/orders/{id}"), None)
            .await
    }
}

#[cfg(test)]
mod tests {
    use httpmock::prelude::*;
    use serde_json::json;

    use super::*;

    fn client(base_url: &str) -> ShoppingClient {
        ShoppingClient::new(base_url, "test-token")
    }

    #[test]
    fn builds_shopping_paths_from_the_api_front_door() {
        assert_eq!(
            client("https://api.test-godaddy.com").url("/catalog/search"),
            "https://api.test-godaddy.com/v1/shopping/catalog/search"
        );
    }

    #[tokio::test]
    async fn surfaces_order_not_found_as_retryable() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(GET).path("/v1/shopping/orders/123");
                then.status(404)
                    .json_body(json!({ "error": "order_not_found" }));
            })
            .await;

        let error = client(&server.base_url())
            .get_order("123")
            .await
            .expect_err("404 is an error");

        mock.assert_async().await;
        assert!(error.is_retryable_order_read());
    }
}
