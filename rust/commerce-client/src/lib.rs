//! GoDaddy Commerce Stores API client (`/v2/commerce/…`).
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
        generated_client_support::notify_request(request);
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
        assert_eq!(store.status, types::StoreStatus::Active);
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
        responses: std::sync::Mutex<Vec<(u16, Vec<u8>)>>,
    }

    impl TransportObserver for RecordingObserver {
        fn on_request(&self, _request: &reqwest::Request) {}

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
