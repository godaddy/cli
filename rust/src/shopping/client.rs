use std::time::Duration;

use shopping_client::types::{
    Checkout, CheckoutCompleteRequest, CheckoutWritableRequest, CompleteCheckoutResponse,
    CreateCheckoutResponse, GetCheckoutResponse, GetOrderResponse, Order, UpdateCheckoutResponse,
};

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
    #[error("Shopping API returned an unexpected error payload: {0:?}")]
    UnexpectedErrorPayload(serde_json::Value),
    #[error("Shopping API returned no data for this request")]
    EmptyResponse,
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
            Self::Network(_)
            | Self::Request(_)
            | Self::Response(_)
            | Self::Build(_)
            | Self::UnexpectedErrorPayload(_)
            | Self::EmptyResponse => None,
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
            ClientError::UnexpectedErrorPayload(payload) => Self::unexpected(format!(
                "Shopping API returned an unexpected payload: {payload}"
            ))
            .with_system("shopping"),
            ClientError::EmptyResponse => {
                Self::unexpected("Shopping API returned no data for this request")
                    .with_system("shopping")
            }
        }
    }
}

/// Builds a generated Shopping API client carrying the caller's authorization
/// and a request ID that correlates every HTTP call this client instance
/// makes. Build one per command invocation, then call its generated
/// operation methods (`search_catalog()`, `get_checkout()`, ...) directly.
pub(crate) fn build_client(
    base_url: impl AsRef<str>,
    token: impl AsRef<str>,
) -> Result<shopping_client::Client, ClientError> {
    shopping_client::client_with_auth(
        base_url.as_ref(),
        &format!("Bearer {}", token.as_ref()),
        USER_AGENT,
        &uuid::Uuid::new_v4().to_string(),
    )
    .map_err(ClientError::Build)
}

/// Decodes a generated operation's response, preserving the caller's typed
/// response shape. `None` means the server returned a successful but empty
/// body (some environments 202/204 certain checkout operations).
pub(crate) async fn decode<T>(
    response: Result<progenitor_client::ResponseValue<T>, progenitor_client::Error<()>>,
) -> Result<Option<T>, ClientError>
where
    T: serde::de::DeserializeOwned,
{
    match response {
        Ok(response) => Ok(Some(response.into_inner())),
        Err(progenitor_client::Error::InvalidResponsePayload(bytes, _)) if bytes.is_empty() => {
            Ok(None)
        }
        Err(progenitor_client::Error::InvalidResponsePayload(_, error)) => {
            // Progenitor already tried to decode this exact payload into `T`
            // inside `.send()` and failed with `error` — retrying the same
            // bytes against the same type here would just fail identically,
            // so surface that original decode error instead.
            Err(ClientError::Request(format!(
                "response did not match the expected shape: {error}"
            )))
        }
        Err(progenitor_client::Error::UnexpectedResponse(response))
            if response.status().is_success() =>
        {
            let bytes = response
                .bytes()
                .await
                .map_err(|error| ClientError::Network(error.to_string()))?;
            if bytes.is_empty() {
                Ok(None)
            } else {
                serde_json::from_slice(&bytes)
                    .map(Some)
                    .map_err(ClientError::Response)
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

/// Every checkout write operation's response is `oneOf [Checkout,
/// ErrorResponse]` (`ErrorResponse` wraps an open-ended JSON error payload).
/// Since every `Checkout` field is optional, untagged deserialization
/// resolves to the `Checkout` variant for any realistic response, so the
/// `ErrorResponse` arm below is defense-in-depth, not a path exercised by
/// well-formed API responses.
trait IntoCheckout {
    fn into_checkout(self) -> Result<Checkout, ClientError>;
}

macro_rules! impl_into_checkout {
    ($($response:ty),+ $(,)?) => {
        $(impl IntoCheckout for $response {
            fn into_checkout(self) -> Result<Checkout, ClientError> {
                match self {
                    Self::Checkout(checkout) => Ok(checkout),
                    Self::ErrorResponse(payload) => {
                        Err(ClientError::UnexpectedErrorPayload(payload.into()))
                    }
                }
            }
        })+
    };
}

impl_into_checkout!(
    GetCheckoutResponse,
    CreateCheckoutResponse,
    UpdateCheckoutResponse,
    CompleteCheckoutResponse,
);

/// For a GET fetching an existing resource, an empty successful body is not
/// a meaningful state (unlike the create/complete mutations below, where
/// it's a documented possible ack) — surface it as an error rather than
/// manufacturing a default value that would misrepresent "no data" as "an
/// empty but real resource."
fn checkout_or_empty_error<T: IntoCheckout>(response: Option<T>) -> Result<Checkout, ClientError> {
    response.map_or(Err(ClientError::EmptyResponse), IntoCheckout::into_checkout)
}

/// A mutation's empty successful body (some environments 202/204 certain
/// checkout operations) is a real, distinct outcome from "no data" — the
/// caller must be able to tell it apart from an actual `Checkout`, so this
/// preserves it as `None` rather than defaulting to an empty `Checkout`.
fn checkout_or_none<T: IntoCheckout>(response: Option<T>) -> Result<Option<Checkout>, ClientError> {
    response.map(IntoCheckout::into_checkout).transpose()
}

pub(crate) async fn get_checkout(
    client: &shopping_client::Client,
    id: &str,
) -> Result<Checkout, ClientError> {
    checkout_or_empty_error(
        decode::<GetCheckoutResponse>(client.get_checkout().id(id).send().await).await?,
    )
}

pub(crate) async fn create_checkout(
    client: &shopping_client::Client,
    body: CheckoutWritableRequest,
    idempotency_key: &str,
) -> Result<Option<Checkout>, ClientError> {
    checkout_or_none(
        decode::<CreateCheckoutResponse>(
            client
                .create_checkout()
                .idempotency_key(idempotency_key)
                .body(body)
                .send()
                .await,
        )
        .await?,
    )
}

pub(crate) async fn update_checkout(
    client: &shopping_client::Client,
    id: &str,
    body: CheckoutWritableRequest,
    idempotency_key: &str,
) -> Result<Option<Checkout>, ClientError> {
    checkout_or_none(
        decode::<UpdateCheckoutResponse>(
            client
                .update_checkout()
                .id(id)
                .idempotency_key(idempotency_key)
                .body(body)
                .send()
                .await,
        )
        .await?,
    )
}

pub(crate) async fn complete_checkout(
    client: &shopping_client::Client,
    id: &str,
    body: CheckoutCompleteRequest,
    idempotency_key: &str,
) -> Result<Option<Checkout>, ClientError> {
    checkout_or_none(
        decode::<CompleteCheckoutResponse>(
            client
                .complete_checkout()
                .id(id)
                .idempotency_key(idempotency_key)
                .body(body)
                .send()
                .await,
        )
        .await?,
    )
}

pub(crate) async fn get_order(
    client: &shopping_client::Client,
    order_id: &str,
) -> Result<Order, ClientError> {
    match decode::<GetOrderResponse>(client.get_order().id(order_id).send().await).await? {
        Some(GetOrderResponse::Order(order)) => Ok(order),
        Some(GetOrderResponse::ErrorResponse(payload)) => {
            Err(ClientError::UnexpectedErrorPayload(payload.into()))
        }
        None => Err(ClientError::EmptyResponse),
    }
}

#[cfg(test)]
mod tests {
    use httpmock::prelude::*;
    use serde_json::json;
    use shopping_client::types::{
        CatalogLookupGetProductRequest, CatalogLookupLookupRequest, CatalogSearchSearchRequest,
        CheckoutCompleteRequest, CheckoutWritableRequest, CompleteCheckoutResponse,
        CreateCheckoutResponse, GetCheckoutResponse, GetOrderResponse, GetProductRequest,
        GetProductResponse, LookupRequest, SearchCatalogResponse, SearchRequest,
        UpdateCheckoutResponse,
    };
    use std::result::Result as TestResult;

    use super::*;

    fn client(base_url: &str) -> shopping_client::Client {
        build_client(base_url, "test-token").expect("client should build")
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
        let search_body = SearchRequest(CatalogSearchSearchRequest {
            query: Some("hosting".to_owned()),
            ..Default::default()
        });
        let lookup_body = LookupRequest(CatalogLookupLookupRequest {
            ids: vec!["web-hosting".to_owned()],
            ..Default::default()
        });
        let product_body = GetProductRequest(CatalogLookupGetProductRequest {
            id: Some("web-hosting".to_owned()),
            ..Default::default()
        });
        decode(shopping.search_catalog().body(search_body).send().await)
            .await
            .expect("search");
        decode(shopping.lookup_catalog().body(lookup_body).send().await)
            .await
            .expect("lookup");
        decode(shopping.get_product().body(product_body).send().await)
            .await
            .expect("product");

        search.assert_async().await;
        lookup.assert_async().await;
        product.assert_async().await;
    }

    #[tokio::test]
    async fn catalog_product_preserves_the_description_field_through_the_typed_client() {
        let server = MockServer::start_async().await;
        let upstream_response = json!({
            "product": {
                "id": "web-hosting",
                "title": "Web Hosting",
                "description": {"plain": "Fast, reliable hosting for your site."}
            }
        });
        let mock = server
            .mock_async(|when, then| {
                when.method(POST).path("/v1/shopping/catalog/product");
                then.status(200).json_body(upstream_response.clone());
            })
            .await;

        let product_body = GetProductRequest(CatalogLookupGetProductRequest {
            id: Some("web-hosting".to_owned()),
            ..Default::default()
        });
        let product: GetProductResponse = decode(
            client(&server.base_url())
                .get_product()
                .body(product_body)
                .send()
                .await,
        )
        .await
        .expect("product")
        .expect("non-empty product response");
        let product = match product {
            GetProductResponse::CatalogLookupGetProductResponse(product) => Some(product),
            GetProductResponse::ErrorResponse(_) => None,
        }
        .expect("expected a product response, not an error payload");

        mock.assert_async().await;
        // Regression guard: a spec-generation bug once stripped the
        // `description` field from the `product`/`variant` schemas (it
        // collided with a documentation-pruning keyword), so progenitor
        // generated a `Product` struct with no `description` field at all,
        // and it silently vanished on the way through the typed client.
        assert_eq!(
            product
                .product
                .expect("product")
                .description
                .expect("description")
                .plain,
            Some("Fast, reliable hosting for your site.".to_owned())
        );
    }

    #[tokio::test]
    async fn checkout_lifecycle_uses_expected_methods_paths_bodies_and_headers() {
        let server = MockServer::start_async().await;
        let checkout_request = json!({
            "line_items": [{"item": {"id": "variant-1"}, "quantity": 1}],
            "payment": {"instruments": [{"id": "instrument-1", "selected": true}]}
        });
        let completion_request = json!({
            "payment": {"instruments": [{
                "id": "instrument-1",
                "selected": true,
                "billing_address": {
                    "street_address": "123 Main St",
                    "address_locality": "Mountain View",
                    "address_country": "US"
                }
            }]},
            "consent": {
                "agreement_types": ["terms", "ssl"],
                "agreed_at": "2026-09-15T12:00:00Z"
            }
        });
        let checkout_body: CheckoutWritableRequest =
            serde_json::from_value(checkout_request.clone()).expect("valid checkout request");
        let completion_body: CheckoutCompleteRequest =
            serde_json::from_value(completion_request.clone()).expect("valid completion request");
        assert_eq!(
            serde_json::to_value(&completion_body).expect("serialize completion request"),
            completion_request
        );
        let create = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v1/shopping/checkout-sessions")
                    .header("authorization", "Bearer test-token")
                    .header_exists("x-request-id")
                    .header("idempotency-key", "customer-key");
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
                    .header_exists("x-request-id")
                    .header("idempotency-key", "customer-key");
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
        decode::<CreateCheckoutResponse>(
            shopping
                .create_checkout()
                .idempotency_key("customer-key")
                .body(checkout_body.clone())
                .send()
                .await,
        )
        .await
        .expect("create");
        decode::<GetCheckoutResponse>(shopping.get_checkout().id("checkout-123").send().await)
            .await
            .expect("get");
        decode::<UpdateCheckoutResponse>(
            shopping
                .update_checkout()
                .id("checkout-123")
                .idempotency_key("customer-key")
                .body(checkout_body)
                .send()
                .await,
        )
        .await
        .expect("update");
        decode::<CompleteCheckoutResponse>(
            shopping
                .complete_checkout()
                .id("checkout-123")
                .idempotency_key("customer-key")
                .body(completion_body)
                .send()
                .await,
        )
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

        let order = decode::<GetOrderResponse>(
            client(&server.base_url())
                .get_order()
                .id("order-123")
                .send()
                .await,
        )
        .await
        .expect("get order")
        .expect("non-empty order response");

        mock.assert_async().await;
        assert!(
            matches!(order, GetOrderResponse::Order(order) if order.id == Some("order-123".to_owned()))
        );
    }

    #[test]
    fn documents_that_an_error_shaped_payload_resolves_to_the_success_variant() {
        // `error_response` has no properties and `Checkout` has none required,
        // so untagged deserialization matches `Checkout` first for *any*
        // JSON object, including a genuine API error — the `ErrorResponse`
        // arms throughout this module are defense-in-depth for a non-object
        // payload, not the primary way this API's in-band errors get caught.
        let error_payload = json!({"error": "something went wrong", "code": "invalid_checkout"});
        let decoded: GetCheckoutResponse =
            serde_json::from_value(error_payload).expect("should decode as *something*");
        assert!(matches!(decoded, GetCheckoutResponse::Checkout(_)));
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
        assert!(
            decode::<CreateCheckoutResponse>(
                shopping
                    .create_checkout()
                    .idempotency_key("customer-key")
                    .body(CheckoutWritableRequest(Default::default()))
                    .send()
                    .await,
            )
            .await
            .expect("empty create")
            .is_none()
        );
        assert!(
            decode::<CompleteCheckoutResponse>(
                shopping
                    .complete_checkout()
                    .id("checkout-123")
                    .idempotency_key("customer-key")
                    .body(CheckoutCompleteRequest(Default::default()))
                    .send()
                    .await,
            )
            .await
            .expect("empty completion")
            .is_none()
        );

        create.assert_async().await;
        complete.assert_async().await;
    }

    #[tokio::test]
    async fn empty_mutation_acks_are_distinct_none_not_a_default_checkout() {
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
        let created = create_checkout(
            &shopping,
            CheckoutWritableRequest(Default::default()),
            "customer-key",
        )
        .await
        .expect("empty create should not be an error");
        let completed = complete_checkout(
            &shopping,
            "checkout-123",
            CheckoutCompleteRequest(Default::default()),
            "customer-key",
        )
        .await
        .expect("empty completion should not be an error");

        create.assert_async().await;
        complete.assert_async().await;
        assert!(
            created.is_none(),
            "an empty ack must stay distinguishable from an empty-but-real Checkout"
        );
        assert!(
            completed.is_none(),
            "an empty ack must stay distinguishable from an empty-but-real Checkout"
        );
    }

    #[tokio::test]
    async fn get_checkout_and_get_order_reject_an_empty_body_as_unexpected() {
        let server = MockServer::start_async().await;
        let checkout = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/v1/shopping/checkout-sessions/checkout-123");
                then.status(200).body("");
            })
            .await;
        let order = server
            .mock_async(|when, then| {
                when.method(GET).path("/v1/shopping/orders/order-123");
                then.status(200).body("");
            })
            .await;

        let shopping = client(&server.base_url());
        let checkout_error = get_checkout(&shopping, "checkout-123")
            .await
            .expect_err("an empty checkout GET is not a valid checkout");
        let order_error = get_order(&shopping, "order-123")
            .await
            .expect_err("an empty order GET is not a valid order");

        checkout.assert_async().await;
        order.assert_async().await;
        assert!(matches!(checkout_error, ClientError::EmptyResponse));
        assert!(matches!(order_error, ClientError::EmptyResponse));
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
        decode::<GetCheckoutResponse>(shopping.get_checkout().id("session/a?b#c%d").send().await)
            .await
            .expect("encoded checkout");
        decode::<GetOrderResponse>(shopping.get_order().id("order/a?b#c%d").send().await)
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

        let error = decode::<GetOrderResponse>(
            client(&server.base_url())
                .get_order()
                .id("order-123")
                .send()
                .await,
        )
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

        let error = decode::<GetOrderResponse>(
            client(&server.base_url())
                .get_order()
                .id("123")
                .send()
                .await,
        )
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

        let error = decode::<UpdateCheckoutResponse>(
            client(&server.base_url())
                .update_checkout()
                .id("checkout-123")
                .idempotency_key("customer-key")
                .body(CheckoutWritableRequest(Default::default()))
                .send()
                .await,
        )
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

        let error = decode::<GetOrderResponse>(
            client(&server.base_url())
                .get_order()
                .id("order-123")
                .send()
                .await,
        )
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

        let error = decode::<SearchCatalogResponse>(
            client(&server.base_url())
                .search_catalog()
                .body(SearchRequest(Default::default()))
                .send()
                .await,
        )
        .await
        .expect_err("malformed JSON is an error");

        mock.assert_async().await;
        assert!(
            matches!(error, ClientError::Request(ref message) if message.contains("expected ident")),
            "expected decode error, received {error}",
        );
    }
}
