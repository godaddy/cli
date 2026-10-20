//! Typed client generated from the vendored Shopping OpenAPI contract.
//!
//! `BuildError`/`TransportObserver`/`set_transport_observer` are shared with
//! every other generated client crate — see `generated-client-support`.

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
/// [`TransportObserver`], if any
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
        generated_client_support::execute_and_observe(self.client(), request).await
    }
}

/// Builds a [`Client`] whose every request carries a pre-set `Authorization`
/// header and `x-request-id`.
///
/// `builder` carries the host application's shared transport policy
/// (user-agent, timeouts, default headers); `authorization` is the full header
/// value the API expects, e.g. `"Bearer <token>"`. The builder is a `reqwest`
/// type, so the caller's `reqwest` must resolve to the same version as this
/// crate's.
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

    fn client_for(server: &MockServer) -> Client {
        client_with_builder(
            reqwest::Client::builder(),
            &server.base_url(),
            "Bearer tok",
            "req-1",
        )
        .expect("build client")
    }

    // Product and variant `metadata` are open objects in the contract: the
    // API returns merchant-defined keys (`subscription_type`, `term_type`, ...)
    // that the schema does not name. The shopping commands print the typed
    // response as JSON, so those keys must survive a decode/encode round trip.
    #[test]
    fn catalog_metadata_keeps_keys_the_contract_does_not_name() {
        let payload = json!({
            "products": [{
                "id": "T1",
                "metadata": {
                    "product_family": "email",
                    "mailbox_size_mb": 10240,
                    "subscription_type": "PAID"
                },
                "variants": [{
                    "id": "T1:1mo",
                    "metadata": {
                        "default": true,
                        "number_of_terms": 1,
                        "term_type": "MONTH"
                    }
                }]
            }]
        });

        let response: types::CatalogSearchSearchResponse =
            serde_json::from_value(payload).expect("decode response");
        let output = serde_json::to_value(&response).expect("encode response");

        let product = &output["products"][0]["metadata"];
        assert_eq!(product["subscription_type"], "PAID");
        assert_eq!(product["mailbox_size_mb"], 10240, "named fields still map");
        let variant = &output["products"][0]["variants"][0]["metadata"];
        assert_eq!(variant["number_of_terms"], 1);
        assert_eq!(variant["term_type"], "MONTH");
        assert_eq!(variant["default"], true, "named fields still map");
    }

    #[derive(Debug, Default)]
    struct RecordingObserver {
        requests: std::sync::Mutex<Vec<String>>,
        responses: std::sync::Mutex<Vec<(u16, Vec<u8>)>>,
    }

    impl TransportObserver for RecordingObserver {
        fn on_request(&self, request: &reqwest::Request) {
            self.requests
                .lock()
                .expect("lock is never held across a panic")
                .push(request.method().to_string());
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

    // Serializes tests that mutate the process-wide transport observer. An
    // async-aware lock, not a `std::sync::Mutex` — the guard is held across
    // this test's `.await` points (clippy::await_holding_lock), which is only
    // sound with a lock that yields the executor instead of blocking a thread.
    static TRANSPORT_OBSERVER_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    struct ClearTransportObserver;
    impl Drop for ClearTransportObserver {
        fn drop(&mut self) {
            set_transport_observer(None);
        }
    }

    // Shopping had no `TransportObserver`/`ClientHooks` at all before this
    // crate started sharing `generated-client-support` with
    // `domains-client`/`email-client` — this is the first proof `--debug
    // transport` logging actually reaches a shopping request/response.
    #[tokio::test]
    async fn client_hooks_feed_request_and_response_events_to_the_registered_observer() {
        let _test_lock = TRANSPORT_OBSERVER_TEST_LOCK.lock().await;
        let _clear = ClearTransportObserver;

        let observer = std::sync::Arc::new(RecordingObserver::default());
        set_transport_observer(Some(observer.clone()));

        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(GET).path("/v1/shopping/orders/order-1");
                then.status(200).json_body(json!({ "id": "order-1" }));
            })
            .await;

        client_for(&server)
            .get_order()
            .id("order-1")
            .send()
            .await
            .expect("request succeeds")
            .into_inner();
        mock.assert_async().await;

        let requests = observer
            .requests
            .lock()
            .expect("lock is never held across a panic");
        assert!(
            requests.iter().any(|m| m == "GET"),
            "expected a request event, got: {requests:?}"
        );
        let responses = observer
            .responses
            .lock()
            .expect("lock is never held across a panic");
        assert!(
            responses.iter().any(|(status, body)| *status == 200
                && String::from_utf8_lossy(body).contains("order-1")),
            "expected a response event carrying the real body, got: {responses:?}"
        );
    }
}
