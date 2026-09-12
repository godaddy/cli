use std::time::Duration;

use serde_json::Value;

const USER_AGENT: &str = concat!("godaddy-cli/", env!("CARGO_PKG_VERSION"));
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("HTTP error {status}: {body}")]
    Http {
        status: u16,
        body: String,
        retry_after: Option<Duration>,
    },
    #[error("network error: {0}")]
    Network(String),
    #[error("request error: {0}")]
    Request(String),
    #[error("failed to decode Shopping API response: {0}")]
    Response(#[from] serde_json::Error),
    #[error("failed to construct Shopping API client: {0}")]
    Build(#[from] shopping_client::BuildError),
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
            Self::Network(_) | Self::Request(_) | Self::Response(_) | Self::Build(_) => None,
        }
    }
}

impl From<ClientError> for crate::error::GddyError {
    fn from(value: ClientError) -> Self {
        match value {
            ClientError::Http { status, body, .. } => Self::from_http(status, body, "shopping"),
            ClientError::Network(error) | ClientError::Request(error) => {
                Self::network(error).with_system("shopping")
            }
            ClientError::Response(error) => {
                Self::unexpected(format!("failed to decode Shopping API response: {error}"))
                    .with_system("shopping")
            }
            ClientError::Build(error) => {
                Self::config(format!("failed to construct Shopping API client: {error}"))
                    .with_system("shopping")
            }
        }
    }
}

pub struct ShoppingClient {
    base_url: String,
    authorization: String,
}

impl ShoppingClient {
    pub fn new(base_url: impl AsRef<str>, token: impl AsRef<str>) -> Result<Self, ClientError> {
        let client = Self {
            base_url: base_url.as_ref().to_owned(),
            authorization: format!("Bearer {}", token.as_ref()),
        };
        client.client()?;
        Ok(client)
    }

    fn client(&self) -> Result<shopping_client::Client, ClientError> {
        shopping_client::client_with_auth(
            &self.base_url,
            &self.authorization,
            USER_AGENT,
            &uuid::Uuid::new_v4().to_string(),
        )
        .map_err(ClientError::Build)
    }

    pub async fn catalog_search(&self, body: Value) -> Result<Value, ClientError> {
        let body: shopping_client::types::SearchRequest = deserialize(body)?;
        response(self.client()?.search_catalog().body(body).send().await).await
    }

    pub async fn catalog_lookup(&self, body: Value) -> Result<Value, ClientError> {
        let body: shopping_client::types::LookupRequest = deserialize(body)?;
        response(self.client()?.lookup_catalog().body(body).send().await).await
    }

    pub async fn catalog_product(&self, body: Value) -> Result<Value, ClientError> {
        let body: shopping_client::types::GetProductRequest = deserialize(body)?;
        response(self.client()?.get_product().body(body).send().await).await
    }

    pub async fn create_checkout(&self, body: Value) -> Result<Value, ClientError> {
        let body: shopping_client::types::CheckoutWritableRequest = deserialize(body)?;
        response(self.client()?.create_checkout().body(body).send().await).await
    }

    pub async fn get_checkout(&self, id: &str) -> Result<Value, ClientError> {
        response(self.client()?.get_checkout().id(id).send().await).await
    }

    pub async fn update_checkout(&self, id: &str, body: Value) -> Result<Value, ClientError> {
        let body: shopping_client::types::CheckoutWritableRequest = deserialize(body)?;
        response(
            self.client()?
                .update_checkout()
                .id(id)
                .body(body)
                .send()
                .await,
        )
        .await
    }

    pub async fn complete_checkout(
        &self,
        id: &str,
        body: Value,
        idempotency_key: &str,
    ) -> Result<Value, ClientError> {
        let body: shopping_client::types::CheckoutCompleteRequest = deserialize(body)?;
        response(
            self.client()?
                .complete_checkout()
                .id(id)
                .idempotency_key(idempotency_key)
                .body(body)
                .send()
                .await,
        )
        .await
    }

    pub async fn get_order(&self, id: &str) -> Result<Value, ClientError> {
        response(self.client()?.get_order().id(id).send().await).await
    }
}

fn deserialize<T: serde::de::DeserializeOwned>(body: Value) -> Result<T, ClientError> {
    serde_json::from_value(body).map_err(ClientError::Response)
}

async fn response<T: serde::Serialize>(
    response: Result<progenitor_client::ResponseValue<T>, progenitor_client::Error<()>>,
) -> Result<Value, ClientError> {
    match response {
        Ok(response) => serde_json::to_value(response.into_inner()).map_err(ClientError::Response),
        Err(progenitor_client::Error::InvalidResponsePayload(bytes, _)) if bytes.is_empty() => {
            Ok(Value::Null)
        }
        Err(progenitor_client::Error::InvalidResponsePayload(bytes, _)) => {
            // UCP extension metadata can evolve independently of the core
            // response schemas. Preserve a successful JSON response for the
            // CLI's dynamic projection when typed decoding cannot represent it.
            serde_json::from_slice(&bytes).map_err(ClientError::Response)
        }
        Err(progenitor_client::Error::UnexpectedResponse(response))
            if response.status().is_success() =>
        {
            let bytes = response.bytes().await.unwrap_or_default();
            if bytes.is_empty() {
                Ok(Value::Null)
            } else {
                serde_json::from_slice(&bytes).map_err(ClientError::Response)
            }
        }
        Err(progenitor_client::Error::UnexpectedResponse(response)) => {
            let status = response.status().as_u16();
            let retry_after = response
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<u64>().ok())
                .map(Duration::from_secs);
            let body = response.text().await.unwrap_or_default();
            Err(ClientError::Http {
                status,
                body,
                retry_after,
            })
        }
        Err(progenitor_client::Error::CommunicationError(error)) => {
            Err(ClientError::Network(error.to_string()))
        }
        Err(error) => Err(ClientError::Request(error.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use httpmock::prelude::*;
    use serde_json::json;
    use std::result::Result as TestResult;

    use super::*;

    fn client(base_url: &str) -> ShoppingClient {
        ShoppingClient::new(base_url, "test-token").expect("client should build")
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
            error => Err(format!("expected HTTP error, received {error}")),
        }
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
                then.status(200).json_body(json!({"products": []}));
            })
            .await;
        let lookup = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v1/shopping/catalog/lookup")
                    .header("authorization", "Bearer test-token")
                    .header_exists("x-request-id")
                    .json_body(lookup_request.clone());
                then.status(200).json_body(json!({"products": []}));
            })
            .await;
        let product = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v1/shopping/catalog/product")
                    .header("authorization", "Bearer test-token")
                    .header_exists("x-request-id")
                    .json_body(product_request.clone());
                then.status(200).json_body(json!({"products": []}));
            })
            .await;

        let shopping = client(&server.base_url());
        shopping
            .catalog_search(search_request)
            .await
            .expect("search");
        shopping
            .catalog_lookup(lookup_request)
            .await
            .expect("lookup");
        shopping
            .catalog_product(product_request)
            .await
            .expect("product");

        search.assert_async().await;
        lookup.assert_async().await;
        product.assert_async().await;
    }

    #[tokio::test]
    async fn checkout_lifecycle_uses_expected_methods_paths_bodies_and_headers() {
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
                    .header_exists("x-request-id");
                then.status(201).json_body(json!({"id": "checkout-123"}));
            })
            .await;
        let get = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/v1/shopping/checkout-sessions/checkout-123")
                    .header("authorization", "Bearer test-token")
                    .header_exists("x-request-id");
                then.status(200).json_body(json!({"id": "checkout-123"}));
            })
            .await;
        let update = server
            .mock_async(|when, then| {
                when.method(PUT)
                    .path("/v1/shopping/checkout-sessions/checkout-123")
                    .header("authorization", "Bearer test-token")
                    .header_exists("x-request-id");
                then.status(200).json_body(json!({"id": "checkout-123"}));
            })
            .await;
        let complete = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v1/shopping/checkout-sessions/checkout-123/complete")
                    .header("authorization", "Bearer test-token")
                    .header_exists("x-request-id")
                    .header("idempotency-key", "customer-key");
                then.status(200).json_body(json!({"id": "checkout-123"}));
            })
            .await;

        let shopping = client(&server.base_url());
        shopping
            .create_checkout(checkout_request.clone())
            .await
            .expect("create");
        shopping.get_checkout("checkout-123").await.expect("get");
        shopping
            .update_checkout("checkout-123", checkout_request)
            .await
            .expect("update");
        shopping
            .complete_checkout("checkout-123", completion_request, "customer-key")
            .await
            .expect("complete");

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
                    .path("/v1/shopping/checkout-sessions/checkout-123/complete")
                    .header("idempotency-key", "customer-key");
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
                .complete_checkout("checkout-123", json!({}), "customer-key")
                .await
                .expect("empty completion"),
            Value::Null
        );

        create.assert_async().await;
        complete.assert_async().await;
    }

    #[tokio::test]
    async fn encodes_dynamic_ids_on_the_wire() {
        let server = MockServer::start_async().await;
        let checkout = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/v1/shopping/checkout-sessions/session%2Fa%3Fb%23c%25d");
                then.status(200).json_body(json!({"id": "checkout"}));
            })
            .await;
        let order = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/v1/shopping/orders/order%2Fa%3Fb%23c%25d");
                then.status(200).json_body(json!({"id": "order"}));
            })
            .await;

        let shopping = client(&server.base_url());
        shopping
            .get_checkout("session/a?b#c%d")
            .await
            .expect("encoded checkout");
        shopping
            .get_order("order/a?b#c%d")
            .await
            .expect("encoded order");

        checkout.assert_async().await;
        order.assert_async().await;
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
                    .json_body(json!({"error": "order_not_found"}));
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
    async fn reports_malformed_success_json() {
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
            ClientError::Response(error) => assert!(error.to_string().contains("expected ident")),
            error => panic!("expected decode error, received {error}"),
        }
    }
}
