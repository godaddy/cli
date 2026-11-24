//! GoDaddy Commerce Stores API client, spanning two API generations behind one
//! host:
//!
//! * **v2** (`/v2/commerce/…`): the Stores API. Its routes are not served in
//!   every environment yet; an unrouted one answers with an empty 404.
//! * **v1** (`/v1/commerce/stores/{storeId}`): the one v1 read the client keeps,
//!   `get_store_by_id_v1`, which is served. Its generated types are `V1`-prefixed
//!   so they cannot clash with v2's. v1's write operations are private upstream,
//!   so they are not part of the client.
//!
//! The contents of this crate are **generated** by `progenitor` at build
//! time from the vendored OpenAPI 3.0 spec. Construct [`Client`] with
//! [`client_with_builder`] to supply a pre-authenticated `reqwest::Client` (the
//! CLI sets the `Authorization: Bearer <token>` header itself). Run
//! `cargo run -p generate-api-catalog` at the workspace root to refresh the
//! spec.
//!
//! The lint allowances are scoped to the generated module so the hand-written
//! code below (`client_with_builder`, the `ClientHooks` bridge) is still linted
//! normally. `BuildError`/`TransportObserver`/`set_transport_observer` are
//! shared with every other generated client crate — see
//! `generated-client-support`.

/// progenitor-generated client + types. Exempt from the workspace's strict
/// style/rustdoc lints (it's machine-generated); the rest of the crate is not.
mod generated {
    #![allow(clippy::all)]
    #![allow(dead_code)]
    #![allow(unused_imports)]
    #![allow(rustdoc::all)]

    include!(concat!(env!("OUT_DIR"), "/codegen.rs"));
}

pub use generated::*;
pub use generated_client_support::{BuildError, TransportObserver, set_transport_observer};

/// Bridges generated requests/responses into the registered
/// [`TransportObserver`], if any. See `email-client`'s equivalent impl for why
/// this is the sanctioned extension point (progenitor's "auto-ref
/// specialization" over `ClientHooks`), not a hack.
impl progenitor_client::ClientHooks<()> for Client {
    async fn pre<E>(
        &self,
        request: &mut reqwest::Request,
        _info: &progenitor_client::OperationInfo,
    ) -> Result<(), progenitor_client::Error<E>> {
        // Requests such as `updateGovernmentIds` and `createStore` carry
        // government IDs, addresses and payment details, so traces get the
        // method, URL and headers only, never the body.
        generated_client_support::notify_request_metadata_only(request);
        Ok(())
    }

    async fn exec(
        &self,
        request: reqwest::Request,
        _info: &progenitor_client::OperationInfo,
    ) -> reqwest::Result<reqwest::Response> {
        // Store responses can carry government IDs, addresses and payment
        // details, so traces get status and headers only, never the body.
        generated_client_support::execute_and_observe_metadata_only(self.client(), request).await
    }
}

/// Builds a [`Client`] whose every request carries a pre-set `Authorization`
/// header and `x-request-id`.
///
/// `builder` carries the host application's shared transport policy
/// (user-agent, timeouts, default headers); `authorization` is the full header
/// value the API expects, e.g. `"Bearer <token>"`.
///
/// # Errors
///
/// [`BuildError`] if `base_url` is not HTTPS (plain HTTP is allowed only on a
/// loopback host), if a header value is invalid, or if the client cannot be
/// built.
pub fn client_with_builder(
    builder: reqwest::ClientBuilder,
    base_url: &str,
    authorization: &str,
    request_id: &str,
) -> Result<Client, BuildError> {
    generated_client_support::validate_base_url(base_url)?;
    let http = generated_client_support::build_authenticated_http_client(
        builder,
        authorization,
        request_id,
    )?;
    Ok(Client::new_with_client(base_url, http))
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;
    use serde_json::json;

    // These exercise the generated request/response wiring against a mock
    // server: HTTP method, the absolute `/v2/commerce/...` path, the
    // `Authorization` and `x-request-id` headers, and response
    // deserialization. They run entirely offline.

    fn client_for(server: &MockServer) -> Client {
        client_with_builder(
            reqwest::Client::builder(),
            &server.base_url(),
            "Bearer tok",
            "req-1",
        )
        .expect("build client")
    }

    #[tokio::test]
    async fn get_store_by_id_requests_the_v2_path_with_auth_headers() {
        let _lock = OBSERVER_TEST_LOCK.lock().await;
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/v2/commerce/stores/store-1")
                    .header("authorization", "Bearer tok")
                    .header("x-request-id", "req-1");
                then.status(200).json_body(json!({
                    "id": "store-1",
                    "businessId": "biz-1",
                    "displayName": "Shop",
                    "status": "ACTIVE"
                }));
            })
            .await;

        let store = client_for(&server)
            .get_store_by_id()
            .store_id("store-1".to_owned())
            .send()
            .await
            .expect("request succeeds")
            .into_inner();

        mock.assert_async().await;
        assert_eq!(store.display_name, "Shop");
        assert_eq!(store.status, Some(types::StoreStatus::Active));
    }

    /// The generator drops every documented non-2xx response so progenitor's
    /// error type resolves to `()`; a real error response must still surface
    /// with its true status rather than being swallowed.
    #[tokio::test]
    async fn a_non_2xx_response_surfaces_its_status() {
        let _lock = OBSERVER_TEST_LOCK.lock().await;
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(GET).path("/v2/commerce/stores/missing");
                then.status(404).body("not found");
            })
            .await;

        let err = client_for(&server)
            .get_store_by_id()
            .store_id("missing".to_owned())
            .send()
            .await
            .expect_err("404 is an error");

        assert_eq!(err.status().map(|s| s.as_u16()), Some(404));
    }

    #[derive(Default)]
    struct RecordingObserver {
        requests: std::sync::Mutex<Vec<Option<Vec<u8>>>>,
        responses: std::sync::Mutex<Vec<(u16, Vec<u8>)>>,
    }

    impl TransportObserver for RecordingObserver {
        fn on_request(&self, request: &reqwest::Request) {
            self.requests
                .lock()
                .expect("lock is never held across a panic")
                .push(
                    request
                        .body()
                        .and_then(reqwest::Body::as_bytes)
                        .map(<[u8]>::to_vec),
                );
        }

        fn on_response(
            &self,
            status: reqwest::StatusCode,
            _headers: &reqwest::header::HeaderMap,
            body: &[u8],
        ) {
            self.responses
                .lock()
                .expect("lock is never held across a panic")
                .push((status.as_u16(), body.to_vec()));
        }
    }

    struct ClearObserver;
    impl Drop for ClearObserver {
        fn drop(&mut self) {
            set_transport_observer(None);
        }
    }

    // The observer registry is process-wide, so a test that registers an
    // observer would also record the requests other tests make at the same
    // time. Every test that sends a request holds this lock.
    static OBSERVER_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    /// Store responses can carry government IDs, addresses and payment
    /// details, so a `--debug transport` trace must see the status but never
    /// the body.
    #[tokio::test]
    async fn generated_calls_feed_the_observer_without_the_response_body() {
        let _lock = OBSERVER_TEST_LOCK.lock().await;
        let _clear = ClearObserver;
        let observer = std::sync::Arc::new(RecordingObserver::default());
        set_transport_observer(Some(observer.clone()));
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(GET).path("/v2/commerce/stores/store-1");
                then.status(200).json_body(json!({
                    "id": "store-1",
                    "displayName": "Shop",
                    "status": "ACTIVE",
                    "governmentIds": [{ "type": "EIN", "governmentId": "secret-gov-id" }]
                }));
            })
            .await;

        // The decoded result is irrelevant here; only the observation matters.
        let _ = client_for(&server)
            .get_store_by_id()
            .store_id("store-1".to_owned())
            .send()
            .await;

        let responses = observer
            .responses
            .lock()
            .expect("lock is never held across a panic");
        assert_eq!(
            *responses,
            vec![(200, generated_client_support::WITHHELD_BODY.to_vec())],
            "the observer should see the status and a placeholder, never the body"
        );
    }

    /// `updateGovernmentIds` sends government IDs, so a trace must show the
    /// request's method, URL and headers but never its body, while the server
    /// still receives the real body.
    #[tokio::test]
    async fn mutation_requests_reach_the_server_but_not_the_observer() {
        let _lock = OBSERVER_TEST_LOCK.lock().await;
        let _clear = ClearObserver;
        let observer = std::sync::Arc::new(RecordingObserver::default());
        set_transport_observer(Some(observer.clone()));
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(PUT)
                    .path("/v2/commerce/stores/store-1/government-ids")
                    .body_includes("secret-gov-id");
                then.status(200).json_body(json!([]));
            })
            .await;

        // The decoded result is irrelevant here; only the observation matters.
        let _ = client_for(&server)
            .update_government_ids()
            .store_id("store-1".to_owned())
            .body(types::GovernmentIdList(vec![types::GovernmentId {
                created_at: None,
                government_id: "secret-gov-id".to_owned(),
                type_: "EIN".to_owned(),
                updated_at: None,
            }]))
            .send()
            .await;

        mock.assert_async().await;
        let requests = observer
            .requests
            .lock()
            .expect("lock is never held across a panic");
        assert_eq!(
            *requests,
            vec![Some(
                generated_client_support::WITHHELD_REQUEST_BODY.to_vec()
            )],
            "the observer should see a placeholder, never the request body"
        );
    }

    /// progenitor ignores `explode: false` and would send repeated keys for an
    /// array, so the generator turns the parameter into a string and callers
    /// pass one comma-separated value.
    #[tokio::test]
    async fn include_fields_is_sent_as_one_comma_separated_value() {
        let _lock = OBSERVER_TEST_LOCK.lock().await;
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/v2/commerce/stores/store-1")
                    .query_param("includeFields", "address,attributes");
                then.status(200).json_body(json!({
                    "id": "store-1",
                    "businessId": "biz-1",
                    "displayName": "Shop",
                    "status": "ACTIVE"
                }));
            })
            .await;

        client_for(&server)
            .get_store_by_id()
            .store_id("store-1".to_owned())
            .include_fields("address,attributes")
            .send()
            .await
            .expect("request succeeds");

        mock.assert_async().await;
    }

    /// What a v1 store read returns, with the fields a live record carries
    /// (`processor` and `acquirer` as processor names, `currency` as a code).
    fn v1_store_body() -> serde_json::Value {
        json!({
            "id": "6d1f0f4c-8a2e-4a52-9a5b-0f6a3c1d2e4b",
            "businessId": "a1e14cc2-29bd-4e99-a1ad-c426cb0266c1",
            "displayName": "Shop",
            "status": "ACTIVE",
            "currency": "USD",
            "timezone": "America/Los_Angeles",
            "processor": "POYNT",
            "acquirer": "POYNT",
            "mockProcessor": true,
            "fixedLocation": true,
            "attributes": { "a": "b" },
            "createdAt": "2026-09-03T23:23:04Z"
        })
    }

    /// The v1 read is a distinct operation on a distinct path: it must request
    /// `/v1/commerce/...`, not v2's path, and carry the same auth headers.
    #[tokio::test]
    async fn get_store_by_id_v1_requests_the_v1_path_with_auth_headers() {
        let _lock = OBSERVER_TEST_LOCK.lock().await;
        let server = MockServer::start_async().await;
        let v1 = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/v1/commerce/stores/store-1")
                    .header("authorization", "Bearer tok")
                    .header("x-request-id", "req-1");
                then.status(200).json_body(v1_store_body());
            })
            .await;
        let v2 = server
            .mock_async(|when, then| {
                when.method(GET).path("/v2/commerce/stores/store-1");
                then.status(200).json_body(json!({}));
            })
            .await;

        client_for(&server)
            .get_store_by_id_v1()
            .store_id("store-1".to_owned())
            .send()
            .await
            .expect("request succeeds");

        v1.assert_async().await;
        assert_eq!(
            v2.calls_async().await,
            0,
            "the v1 read must not touch the v2 route"
        );
    }

    /// The point of keeping v1: it answers where v2 is not routed. The same
    /// store is a 404 on v2 and a record on v1.
    #[tokio::test]
    async fn the_v1_read_succeeds_where_the_v2_route_is_not_served() {
        let _lock = OBSERVER_TEST_LOCK.lock().await;
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(GET).path("/v2/commerce/stores/store-1");
                then.status(404);
            })
            .await;
        server
            .mock_async(|when, then| {
                when.method(GET).path("/v1/commerce/stores/store-1");
                then.status(200).json_body(v1_store_body());
            })
            .await;
        let client = client_for(&server);

        let v2 = client
            .get_store_by_id()
            .store_id("store-1".to_owned())
            .send()
            .await
            .expect_err("v2 is not routed");
        assert_eq!(v2.status().map(|s| s.as_u16()), Some(404));

        let v1 = client
            .get_store_by_id_v1()
            .store_id("store-1".to_owned())
            .send()
            .await
            .expect("v1 is served")
            .into_inner();
        assert_eq!(v1.display_name.as_deref(), Some("Shop"));
    }

    /// The v1 response decodes into the generated type, including the fields the
    /// spec does not describe well: `businessId` (required but undeclared
    /// upstream) and the processor names.
    #[tokio::test]
    async fn the_v1_response_decodes_into_the_generated_type() {
        let _lock = OBSERVER_TEST_LOCK.lock().await;
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(GET).path("/v1/commerce/stores/store-1");
                then.status(200).json_body(v1_store_body());
            })
            .await;

        let store = client_for(&server)
            .get_store_by_id_v1()
            .store_id("store-1".to_owned())
            .send()
            .await
            .expect("request succeeds")
            .into_inner();

        assert_eq!(
            store.business_id.as_deref(),
            Some("a1e14cc2-29bd-4e99-a1ad-c426cb0266c1"),
            "businessId must survive decoding"
        );
        assert_eq!(store.display_name.as_deref(), Some("Shop"));
        assert_eq!(store.status, Some(types::V1StoreStatus::Active));
        assert_eq!(store.processor, Some(types::V1Processor::Poynt));
        assert_eq!(store.acquirer, Some(types::V1Processor::Poynt));
        assert_eq!(store.timezone.as_deref(), Some("America/Los_Angeles"));
    }

    /// A sparse v1 body must still decode: the v1 schemas are relaxed readers.
    #[tokio::test]
    async fn a_sparse_v1_response_still_decodes() {
        let _lock = OBSERVER_TEST_LOCK.lock().await;
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(GET).path("/v1/commerce/stores/store-1");
                then.status(200)
                    .json_body(json!({ "id": "6d1f0f4c-8a2e-4a52-9a5b-0f6a3c1d2e4b" }));
            })
            .await;

        let store = client_for(&server)
            .get_store_by_id_v1()
            .store_id("store-1".to_owned())
            .send()
            .await
            .expect("a sparse record decodes")
            .into_inner();
        assert!(store.display_name.is_none());
        assert!(store.business_id.is_none());
        assert!(
            store.status.is_none(),
            "a store sent without a status must not read as ACTIVE: {:?}",
            store.status
        );
    }

    /// The same holds on v2, which declared the same default.
    #[tokio::test]
    async fn a_v2_store_without_a_status_does_not_read_as_active() {
        let _lock = OBSERVER_TEST_LOCK.lock().await;
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(GET).path("/v2/commerce/stores/store-1");
                then.status(200)
                    .json_body(json!({ "displayName": "Shop", "businessId": "biz-1" }));
            })
            .await;

        let store = client_for(&server)
            .get_store_by_id()
            .store_id("store-1".to_owned())
            .send()
            .await
            .expect("request succeeds")
            .into_inner();
        assert!(store.status.is_none(), "{:?}", store.status);
    }

    /// v1's field selector is un-exploded in the spec, which progenitor would
    /// serialize as repeated keys; it is a string here, so the caller passes one
    /// comma-separated value.
    #[tokio::test]
    async fn the_v1_field_selector_is_one_comma_separated_value() {
        let _lock = OBSERVER_TEST_LOCK.lock().await;
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/v1/commerce/stores/store-1")
                    .query_param("includeField", "address,attributes");
                then.status(200).json_body(v1_store_body());
            })
            .await;

        client_for(&server)
            .get_store_by_id_v1()
            .store_id("store-1".to_owned())
            .include_field("address,attributes")
            .send()
            .await
            .expect("request succeeds");

        mock.assert_async().await;
    }

    /// A v1 error response keeps its real status, like v2's.
    #[tokio::test]
    async fn a_v1_non_2xx_response_surfaces_its_status() {
        let _lock = OBSERVER_TEST_LOCK.lock().await;
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(GET).path("/v1/commerce/stores/missing");
                then.status(404).body("not found");
            })
            .await;

        let error = client_for(&server)
            .get_store_by_id_v1()
            .store_id("missing".to_owned())
            .send()
            .await
            .expect_err("404 is an error");
        assert_eq!(error.status().map(|s| s.as_u16()), Some(404));
    }

    /// A v1 store carries personal and payment details too, so its body is
    /// withheld from a `--debug transport` trace like v2's.
    #[tokio::test]
    async fn a_v1_response_body_is_withheld_from_the_observer() {
        let _lock = OBSERVER_TEST_LOCK.lock().await;
        let _clear = ClearObserver;
        let observer = std::sync::Arc::new(RecordingObserver::default());
        set_transport_observer(Some(observer.clone()));
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(GET).path("/v1/commerce/stores/store-1");
                then.status(200).json_body(v1_store_body());
            })
            .await;

        let _ = client_for(&server)
            .get_store_by_id_v1()
            .store_id("store-1".to_owned())
            .send()
            .await;

        let responses = observer
            .responses
            .lock()
            .expect("lock is never held across a panic");
        assert_eq!(
            *responses,
            vec![(200, generated_client_support::WITHHELD_BODY.to_vec())]
        );
    }

    #[test]
    fn a_cleartext_non_loopback_base_url_is_rejected() {
        let result = client_with_builder(
            reqwest::Client::builder(),
            "http://api.example.com",
            "Bearer tok",
            "req-1",
        );
        assert!(matches!(
            result,
            Err(BuildError::InsecureBaseUrlScheme { .. })
        ));
    }
}
