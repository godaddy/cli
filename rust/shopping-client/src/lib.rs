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

/// Builds a generated client whose requests carry the caller's authorization
/// and correlation headers.
pub fn client_with_auth(
    base_url: &str,
    authorization: &str,
    user_agent: &str,
    request_id: &str,
) -> Result<Client, BuildError> {
    let http = generated_client_support::build_authenticated_http_client(
        authorization,
        user_agent,
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
        client_with_auth(
            &server.base_url(),
            "Bearer tok",
            "godaddy-cli/test",
            "req-1",
        )
        .expect("build client")
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
