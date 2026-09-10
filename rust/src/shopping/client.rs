use std::time::Duration;

use reqwest::{Client, Method};
use serde_json::{Value, json};

use crate::api_explorer::http::encode_path_segment;
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
        self.send_json(Method::GET, &checkout_path(id, ""), None)
            .await
    }

    pub async fn update_checkout(&self, id: &str, body: Value) -> Result<Value, ClientError> {
        self.send_json(Method::PUT, &checkout_path(id, ""), Some(body))
            .await
    }

    pub async fn complete_checkout(&self, id: &str, body: Value) -> Result<Value, ClientError> {
        self.send_json(Method::POST, &checkout_path(id, "/complete"), Some(body))
            .await
    }

    pub async fn get_order(&self, id: &str) -> Result<Value, ClientError> {
        self.send_json(Method::GET, &order_path(id), None).await
    }
}

fn checkout_path(id: &str, suffix: &str) -> String {
    format!("/checkout-sessions/{}{suffix}", encode_path_segment(id))
}

fn order_path(id: &str) -> String {
    format!("/orders/{}", encode_path_segment(id))
}

#[cfg(test)]
mod tests {
    use httpmock::prelude::*;
    use serde_json::json;
    use std::result::Result as TestResult;

    use super::*;

    fn client(base_url: &str) -> ShoppingClient {
        ShoppingClient::new(base_url, "test-token")
    }

    fn assert_http_error(
        error: ClientError,
        expected_status: u16,
        expected_body: &str,
        expected_retry_after: Option<Duration>,
    ) -> TestResult<(), String> {
        match error {
            ClientError::Http {
                status,
                body,
                retry_after,
            } => {
                assert_eq!(status, expected_status);
                assert_eq!(body, expected_body);
                assert_eq!(retry_after, expected_retry_after);
                Ok(())
            }
            ClientError::Network(error) => Err(format!(
                "expected HTTP error, received network error: {error}"
            )),
        }
    }

    #[test]
    fn builds_shopping_paths_from_the_api_front_door() {
        assert_eq!(
            client("https://api.test-godaddy.com").url("/catalog/search"),
            "https://api.test-godaddy.com/v1/shopping/catalog/search"
        );
    }

    #[test]
    fn encodes_dynamic_ids_as_path_segments() {
        assert_eq!(
            checkout_path("session/a?b#c%d", "/complete"),
            "/checkout-sessions/session%2Fa%3Fb%23c%25d/complete"
        );
        assert_eq!(order_path("order/a?b#c%d"), "/orders/order%2Fa%3Fb%23c%25d");
    }

    #[tokio::test]
    async fn catalog_operations_send_expected_requests() {
        let server = MockServer::start_async().await;
        let search_request = json!({"query": "hosting"});
        let lookup_request = json!({"ids": ["web-hosting"]});
        let product_request = json!({"id": "web-hosting"});
        let search = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v1/shopping/catalog/search")
                    .header("authorization", "Bearer test-token")
                    .header_exists("x-request-id")
                    .json_body(search_request.clone());
                then.status(200).json_body(json!({"operation": "search"}));
            })
            .await;
        let lookup = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v1/shopping/catalog/lookup")
                    .header("authorization", "Bearer test-token")
                    .header_exists("x-request-id")
                    .json_body(lookup_request.clone());
                then.status(200).json_body(json!({"operation": "lookup"}));
            })
            .await;
        let product = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v1/shopping/catalog/product")
                    .header("authorization", "Bearer test-token")
                    .header_exists("x-request-id")
                    .json_body(product_request.clone());
                then.status(200).json_body(json!({"operation": "product"}));
            })
            .await;

        let shopping = client(&server.base_url());
        assert_eq!(
            shopping
                .catalog_search(search_request)
                .await
                .expect("search")["operation"],
            "search"
        );
        assert_eq!(
            shopping
                .catalog_lookup(lookup_request)
                .await
                .expect("lookup")["operation"],
            "lookup"
        );
        assert_eq!(
            shopping
                .catalog_product(product_request)
                .await
                .expect("product")["operation"],
            "product"
        );

        search.assert_async().await;
        lookup.assert_async().await;
        product.assert_async().await;
    }

    #[tokio::test]
    async fn checkout_lifecycle_uses_expected_methods_paths_and_bodies() {
        let server = MockServer::start_async().await;
        let checkout_request = json!({
            "line_items": [{"item": {"id": "variant-1"}, "quantity": 1}],
            "payment": {"instruments": [{"id": "instrument-1", "selected": true}]}
        });
        let completion_request =
            json!({"payment": {"instruments": [{"id": "instrument-1", "selected": true}]}});
        let create = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v1/shopping/checkout-sessions")
                    .header("authorization", "Bearer test-token")
                    .header_exists("x-request-id")
                    .json_body(checkout_request.clone());
                then.status(201)
                    .json_body(json!({"id": "checkout-123", "operation": "create"}));
            })
            .await;
        let get = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/v1/shopping/checkout-sessions/checkout-123")
                    .header("authorization", "Bearer test-token")
                    .header_exists("x-request-id");
                then.status(200)
                    .json_body(json!({"id": "checkout-123", "operation": "get"}));
            })
            .await;
        let update = server
            .mock_async(|when, then| {
                when.method(PUT)
                    .path("/v1/shopping/checkout-sessions/checkout-123")
                    .header("authorization", "Bearer test-token")
                    .header_exists("x-request-id")
                    .json_body(checkout_request.clone());
                then.status(200)
                    .json_body(json!({"id": "checkout-123", "operation": "update"}));
            })
            .await;
        let complete = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v1/shopping/checkout-sessions/checkout-123/complete")
                    .header("authorization", "Bearer test-token")
                    .header_exists("x-request-id")
                    .json_body(completion_request.clone());
                then.status(200)
                    .json_body(json!({"id": "checkout-123", "operation": "complete"}));
            })
            .await;

        let shopping = client(&server.base_url());
        assert_eq!(
            shopping
                .create_checkout(checkout_request.clone())
                .await
                .expect("create")["operation"],
            "create"
        );
        assert_eq!(
            shopping.get_checkout("checkout-123").await.expect("get")["operation"],
            "get"
        );
        assert_eq!(
            shopping
                .update_checkout("checkout-123", checkout_request)
                .await
                .expect("update")["operation"],
            "update"
        );
        assert_eq!(
            shopping
                .complete_checkout("checkout-123", completion_request)
                .await
                .expect("complete")["operation"],
            "complete"
        );

        create.assert_async().await;
        get.assert_async().await;
        update.assert_async().await;
        complete.assert_async().await;
    }

    #[tokio::test]
    async fn order_read_sends_expected_request() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/v1/shopping/orders/order-123")
                    .header("authorization", "Bearer test-token")
                    .header_exists("x-request-id");
                then.status(200).json_body(json!({"id": "order-123"}));
            })
            .await;

        let order = client(&server.base_url())
            .get_order("order-123")
            .await
            .expect("get order");

        mock.assert_async().await;
        assert_eq!(order["id"], "order-123");
    }

    #[tokio::test]
    async fn accepts_empty_success_responses() {
        let server = MockServer::start_async().await;
        let create = server
            .mock_async(|when, then| {
                when.method(POST).path("/v1/shopping/checkout-sessions");
                then.status(202).body("");
            })
            .await;
        let complete = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v1/shopping/checkout-sessions/checkout-123/complete");
                then.status(204);
            })
            .await;

        let shopping = client(&server.base_url());
        assert_eq!(
            shopping
                .create_checkout(json!({}))
                .await
                .expect("empty create"),
            Value::Null
        );
        assert_eq!(
            shopping
                .complete_checkout("checkout-123", json!({}))
                .await
                .expect("empty completion"),
            Value::Null
        );

        create.assert_async().await;
        complete.assert_async().await;
    }

    #[tokio::test]
    async fn encodes_dynamic_checkout_ids_on_the_wire() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/v1/shopping/checkout-sessions/session%2Fa%3Fb%23c%25d")
                    .header("authorization", "Bearer test-token");
                then.status(200).json_body(json!({"id": "encoded"}));
            })
            .await;

        let checkout = client(&server.base_url())
            .get_checkout("session/a?b#c%d")
            .await
            .expect("get encoded checkout");

        mock.assert_async().await;
        assert_eq!(checkout["id"], "encoded");
    }

    #[tokio::test]
    async fn preserves_rate_limit_error_details_for_order_reads() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(GET).path("/v1/shopping/orders/order-123");
                then.status(429)
                    .header("retry-after", "7")
                    .body(r#"{"error":"rate_limited"}"#);
            })
            .await;

        let error = client(&server.base_url())
            .get_order("order-123")
            .await
            .expect_err("429 is an error");

        mock.assert_async().await;
        assert!(error.is_retryable_order_read());
        assert_http_error(
            error,
            429,
            r#"{"error":"rate_limited"}"#,
            Some(Duration::from_secs(7)),
        )
        .expect("expected rate-limit HTTP error");
    }

    #[tokio::test]
    async fn preserves_order_not_found_as_retryable() {
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
        assert_http_error(error, 404, r#"{"error":"order_not_found"}"#, None)
            .expect("expected not-found HTTP error");
    }

    #[tokio::test]
    async fn preserves_non_retryable_http_errors() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(PUT)
                    .path("/v1/shopping/checkout-sessions/checkout-123");
                then.status(400).body(r#"{"error":"invalid_checkout"}"#);
            })
            .await;

        let error = client(&server.base_url())
            .update_checkout("checkout-123", json!({}))
            .await
            .expect_err("400 is an error");

        mock.assert_async().await;
        assert!(!error.is_retryable_order_read());
        assert_http_error(error, 400, r#"{"error":"invalid_checkout"}"#, None)
            .expect("expected validation HTTP error");
    }

    #[tokio::test]
    async fn preserves_empty_server_errors_and_retry_after() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(GET).path("/v1/shopping/orders/order-123");
                then.status(503).header("retry-after", "3");
            })
            .await;

        let error = client(&server.base_url())
            .get_order("order-123")
            .await
            .expect_err("503 is an error");

        mock.assert_async().await;
        assert!(error.is_retryable_order_read());
        assert_http_error(error, 503, "", Some(Duration::from_secs(3)))
            .expect("expected server HTTP error");
    }

    #[tokio::test]
    async fn reports_malformed_success_json() -> TestResult<(), String> {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(POST).path("/v1/shopping/catalog/search");
                then.status(200).body("not-json");
            })
            .await;

        let error = client(&server.base_url())
            .catalog_search(json!({}))
            .await
            .expect_err("malformed JSON is an error");

        mock.assert_async().await;
        match error {
            ClientError::Http {
                status,
                body,
                retry_after,
            } => {
                assert_eq!(status, 200);
                assert!(body.contains("invalid JSON response"));
                assert!(body.contains("not-json"));
                assert_eq!(retry_after, None);
            }
            ClientError::Network(error) => {
                return Err(format!(
                    "expected HTTP error, received network error: {error}"
                ));
            }
        }
        Ok(())
    }
}
