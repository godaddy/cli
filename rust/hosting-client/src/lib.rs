//! Typed client generated from the vendored Hosting OpenAPI contract.

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
/// [`TransportObserver`], if any.
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

    fn build(base_url: &str) -> Result<Client, BuildError> {
        client_with_builder(reqwest::Client::builder(), base_url, "Bearer tok", "req-1")
    }

    #[derive(Default)]
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

    // The observer registry is process-wide; serialize the tests that set it.
    static OBSERVER_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    struct ClearObserver;
    impl Drop for ClearObserver {
        fn drop(&mut self) {
            set_transport_observer(None);
        }
    }

    /// Generated hosting calls are logged through the shared observer with the
    /// real response body. That is why no response schema may carry a secret —
    /// see `no_response_schema_property_looks_like_a_secret`.
    #[tokio::test]
    async fn generated_calls_feed_request_and_response_body_to_the_observer() {
        let _lock = OBSERVER_TEST_LOCK.lock().await;
        let _clear = ClearObserver;
        let observer = std::sync::Arc::new(RecordingObserver::default());
        set_transport_observer(Some(observer.clone()));
        let server = httpmock::MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(httpmock::Method::GET)
                    .path("/v1/hosting/apps/app-1");
                then.status(200)
                    .json_body(serde_json::json!({ "id": "app-1", "marker": "body-marker-123" }));
            })
            .await;
        let client = client_with_builder(
            reqwest::Client::builder(),
            &server.base_url(),
            "Bearer tok",
            "req-1",
        )
        .expect("client builds");

        // The decoded result is irrelevant here; only the observation matters.
        let _ = client.get_app().app_id("app-1").send().await;

        assert_eq!(
            *observer
                .requests
                .lock()
                .expect("lock is never held across a panic"),
            vec!["GET".to_owned()]
        );
        let responses = observer
            .responses
            .lock()
            .expect("lock is never held across a panic");
        assert!(
            responses.iter().any(|(status, body)| *status == 200
                && String::from_utf8_lossy(body).contains("body-marker-123")),
            "expected the response body to reach the observer, got: {responses:?}"
        );
    }

    /// Property names that look secret-ish but carry no secret value: `secrets`
    /// is a list of `Secret` metadata (name and timestamps only).
    const SECRET_LOOKING_BUT_SAFE: &[&str] = &["secrets"];

    /// `--debug transport` prints generated-call response bodies verbatim, and
    /// cli-engine has no body-redaction hook. If a regenerated spec adds a
    /// secret-bearing response field, fail here so it is handled (hand-rolled
    /// call with a redacted trace, like the agent-token mint) before shipping.
    #[test]
    fn no_response_schema_property_looks_like_a_secret() {
        fn collect(node: &serde_json::Value, found: &mut Vec<String>) {
            match node {
                serde_json::Value::Object(map) => {
                    if let Some(serde_json::Value::Object(props)) = map.get("properties") {
                        for name in props.keys() {
                            let lower = name.to_ascii_lowercase();
                            let suspicious = ["token", "secret", "password", "credential", "key"]
                                .iter()
                                .any(|needle| lower.contains(needle));
                            if suspicious && !SECRET_LOOKING_BUT_SAFE.contains(&name.as_str()) {
                                found.push(name.clone());
                            }
                        }
                    }
                    map.values().for_each(|child| collect(child, found));
                }
                serde_json::Value::Array(items) => items.iter().for_each(|i| collect(i, found)),
                _ => {}
            }
        }

        let spec: serde_json::Value =
            serde_json::from_str(include_str!("../openapi/hosting.oas3.json"))
                .expect("vendored spec is valid JSON");
        let mut found = Vec::new();
        collect(&spec["components"]["schemas"], &mut found);

        assert!(
            found.is_empty(),
            "response schema field(s) that look like secrets: {found:?}. Generated calls log \
             response bodies under --debug transport; route these through a hand-rolled call \
             with a redacted trace, or add to SECRET_LOOKING_BUT_SAFE if they carry no secret."
        );
    }

    #[test]
    fn accepts_https_base_url() {
        build("https://api.godaddy.com").expect("https is allowed");
    }

    #[test]
    fn accepts_loopback_http_for_tests() {
        build("http://127.0.0.1:9").expect("loopback http is allowed");
    }

    #[test]
    fn rejects_remote_http_base_url() {
        let error = build("http://example.com").expect_err("remote http is rejected");
        assert!(matches!(
            error,
            BuildError::InsecureBaseUrlScheme { ref scheme } if scheme == "http"
        ));
    }
}
