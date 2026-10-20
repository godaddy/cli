//! Shared hand-written support code for `progenitor`-generated API client
//! crates (`domains-client`, `shopping-client`, `email-client`, ...).
//!
//! `progenitor` generates each crate's `Client` type itself, so the pieces
//! that have to name that type — `Client::new_with_client`, and the
//! `progenitor_client::ClientHooks<()> for Client` impl Rust's orphan rule
//! requires to live alongside `Client` — can't move here. Everything else
//! that every generated client crate re-derives identically lives here
//! instead: the base-URL policy, the authenticated-`reqwest::Client` builder,
//! and the [`TransportObserver`] extension point + its process-wide registry.

use std::sync::{Arc, RwLock};

use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderName, HeaderValue};

/// Error building the authenticated HTTP client underlying a generated
/// `Client`.
#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    #[error("invalid header value: {0}")]
    Header(#[from] reqwest::header::InvalidHeaderValue),
    #[error("failed to build HTTP client: {0}")]
    Http(#[from] reqwest::Error),
    #[error("invalid base URL: {0}")]
    InvalidBaseUrl(#[from] url::ParseError),
    #[error("insecure base URL scheme '{scheme}': HTTPS is required")]
    InsecureBaseUrlScheme { scheme: String },
}

/// Rejects a base URL that would send the caller's bearer token in cleartext:
/// anything but `https`, with `http` allowed only for a loopback host (local
/// mock servers). Every generated client's `client_with_builder` calls this
/// before building, so the policy is defined once.
///
/// # Errors
///
/// [`BuildError::InvalidBaseUrl`] if `base_url` does not parse, and
/// [`BuildError::InsecureBaseUrlScheme`] for a non-HTTPS scheme on a
/// non-loopback host.
pub fn validate_base_url(base_url: &str) -> Result<(), BuildError> {
    let parsed = url::Url::parse(base_url)?;
    match parsed.scheme() {
        "https" => Ok(()),
        "http" if is_loopback_host(parsed.host()) => Ok(()),
        scheme => Err(BuildError::InsecureBaseUrlScheme {
            scheme: scheme.to_owned(),
        }),
    }
}

fn is_loopback_host(host: Option<url::Host<&str>>) -> bool {
    match host {
        Some(url::Host::Domain(domain)) => domain == "localhost",
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    }
}

/// Builds the pre-authenticated `reqwest::Client` a generated client's
/// `client_with_builder` wraps in `Client::new_with_client(base_url, ..)`.
/// `authorization` is the full header value the API expects, e.g.
/// `"Bearer <token>"`.
///
/// `builder` carries the host application's shared transport policy
/// (user-agent, default headers, timeouts); this adds only the per-client
/// `Authorization` and `x-request-id` headers on top. `builder` is a
/// `reqwest` type, so the caller's `reqwest` must resolve to the same version
/// as this crate's.
pub fn build_authenticated_http_client(
    builder: reqwest::ClientBuilder,
    authorization: &str,
    request_id: &str,
) -> Result<reqwest::Client, BuildError> {
    let mut headers = HeaderMap::new();
    headers.insert(AUTHORIZATION, HeaderValue::from_str(authorization)?);
    headers.insert(
        HeaderName::from_static("x-request-id"),
        HeaderValue::from_str(request_id)?,
    );
    Ok(builder.default_headers(headers).build()?)
}

/// Observes every request/response a generated client makes, independent of
/// any particular logging backend. This crate has no compile-time dependency
/// on `cli-engine` (or any other framework) — the *caller* pushes an
/// implementation in via [`set_transport_observer`] rather than this crate
/// pulling one in.
pub trait TransportObserver: Send + Sync {
    fn on_request(&self, request: &reqwest::Request);
    fn on_response(
        &self,
        status: reqwest::StatusCode,
        headers: &reqwest::header::HeaderMap,
        body: &[u8],
    );
}

static TRANSPORT_OBSERVER: RwLock<Option<Arc<dyn TransportObserver>>> = RwLock::new(None);

/// Registers (or clears, with `None`) the process-wide transport observer
/// shared by every generated client crate. The main crate calls this once
/// with an adapter around its own logging framework — e.g. cli-engine's
/// `--debug transport` bridge — before making any request through any
/// generated `Client`.
pub fn set_transport_observer(observer: Option<Arc<dyn TransportObserver>>) {
    *TRANSPORT_OBSERVER
        .write()
        .expect("lock is never held across a panic") = observer;
}

fn current_observer() -> Option<Arc<dyn TransportObserver>> {
    TRANSPORT_OBSERVER
        .read()
        .expect("lock is never held across a panic")
        .clone()
}

/// Forwards a generated request to the registered [`TransportObserver`], if
/// any. Called from each generated client crate's
/// `progenitor_client::ClientHooks::pre` — that impl has to live in the
/// crate that defines `Client` (orphan rule), so this is the one-line body
/// it delegates to.
pub fn notify_request(request: &reqwest::Request) {
    if let Some(observer) = current_observer() {
        observer.on_request(request);
    }
}

/// Executes `request` on `client`, then — if a [`TransportObserver`] is
/// registered — reads the response body, forwards it to the observer, and
/// returns a freshly built `Response` carrying the same status, headers,
/// and bytes, so whatever decodes the response afterward (a generated
/// client's own response handling) is completely unaffected and needs no
/// awareness that logging happened at all.
///
/// Call this from each generated client crate's `ClientHooks::exec`
/// override. `exec` is the hook progenitor's own codegen feeds directly into
/// every operation's response handling (`let response =
/// client.exec(request, &info).await?; match response.status() { ... }`),
/// and the only hook that ever owns the request/response outright — `pre`
/// only gets `&mut Request` and `post` only `&Result<Response>`, so neither
/// can read a body (a body read needs ownership: `Response::bytes` takes
/// `self`).
///
/// One known gap: `reqwest` tracks a response's originating URL via a
/// private extension type, so the rebuilt response's `.url()` reads back a
/// placeholder rather than the real request URL. Nothing in this workspace
/// calls `Response::url()` today.
pub async fn execute_and_observe(
    client: &reqwest::Client,
    request: reqwest::Request,
) -> reqwest::Result<reqwest::Response> {
    let response = client.execute(request).await?;

    let Some(observer) = current_observer() else {
        return Ok(response);
    };

    let status = response.status();
    let version = response.version();
    let headers = response.headers().clone();
    let bytes = response.bytes().await?;
    observer.on_response(status, &headers, &bytes);

    let mut builder = http::Response::builder().status(status).version(version);
    if let Some(rebuilt_headers) = builder.headers_mut() {
        *rebuilt_headers = headers;
    }
    let rebuilt = builder
        .body(bytes)
        .expect("status/headers copied from a real response are always valid");
    Ok(rebuilt.into())
}

/// Reported to the [`TransportObserver`] in place of a response body that
/// [`execute_and_observe_metadata_only`] withholds.
pub const WITHHELD_BODY: &[u8] = b"<redacted: response body withheld>";

/// Like [`execute_and_observe`], but the observer sees only the status and
/// headers, with [`WITHHELD_BODY`] in place of the body. The body is never read
/// here and the response is returned untouched.
///
/// For APIs whose responses can carry free-form or secret content (log lines,
/// minted credentials) that must not reach a `--debug transport` trace.
pub async fn execute_and_observe_metadata_only(
    client: &reqwest::Client,
    request: reqwest::Request,
) -> reqwest::Result<reqwest::Response> {
    let response = client.execute(request).await?;
    if let Some(observer) = current_observer() {
        observer.on_response(response.status(), response.headers(), WITHHELD_BODY);
    }
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_authenticated_http_client_rejects_an_invalid_header_value() {
        let err =
            build_authenticated_http_client(reqwest::Client::builder(), "Bearer tok\n", "req-1")
                .expect_err("a header value containing a newline is invalid");
        assert!(matches!(err, BuildError::Header(_)));
    }

    #[test]
    fn build_authenticated_http_client_succeeds_with_valid_inputs() {
        build_authenticated_http_client(reqwest::Client::builder(), "Bearer tok", "req-1")
            .expect("valid inputs build a client");
    }

    #[test]
    fn validate_base_url_accepts_https() {
        validate_base_url("https://api.example.test").expect("https is allowed");
    }

    #[test]
    fn validate_base_url_accepts_http_only_on_loopback() {
        for url in [
            "http://localhost:8080",
            "http://127.0.0.1:8080",
            "http://[::1]:8080",
        ] {
            validate_base_url(url).unwrap_or_else(|e| panic!("{url} should be allowed: {e}"));
        }
    }

    #[test]
    fn validate_base_url_rejects_remote_http_and_lookalike_hosts() {
        for url in [
            "http://api.example.test",
            "http://localhost.example.test",
            "http://127.0.0.1.example.test",
            "http://10.0.0.1",
        ] {
            let err = validate_base_url(url).expect_err("remote http must be rejected");
            assert!(
                matches!(&err, BuildError::InsecureBaseUrlScheme { scheme } if scheme == "http"),
                "{url}: {err}"
            );
        }
    }

    #[test]
    fn validate_base_url_rejects_other_schemes_and_garbage() {
        assert!(matches!(
            validate_base_url("ftp://localhost").expect_err("ftp"),
            BuildError::InsecureBaseUrlScheme { .. }
        ));
        assert!(matches!(
            validate_base_url("not a url").expect_err("garbage"),
            BuildError::InvalidBaseUrl(_)
        ));
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

    // Serializes tests that mutate the process-wide observer
    static TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    struct ClearObserver;
    impl Drop for ClearObserver {
        fn drop(&mut self) {
            set_transport_observer(None);
        }
    }

    #[tokio::test]
    async fn notify_request_reaches_the_registered_observer() {
        let _guard = TEST_LOCK.lock().await;
        let _clear = ClearObserver;

        let observer = Arc::new(RecordingObserver::default());
        set_transport_observer(Some(observer.clone()));

        let request = reqwest::Request::new(
            reqwest::Method::GET,
            "https://example.test".parse().expect("valid url"),
        );
        notify_request(&request);

        assert_eq!(
            *observer
                .requests
                .lock()
                .expect("lock is never held across a panic"),
            vec!["GET".to_owned()]
        );
    }

    #[tokio::test]
    async fn execute_and_observe_forwards_the_real_body_and_still_lets_the_caller_read_it() {
        let _guard = TEST_LOCK.lock().await;
        let _clear = ClearObserver;

        let observer = Arc::new(RecordingObserver::default());
        set_transport_observer(Some(observer.clone()));

        let server = httpmock::MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::GET).path("/probe");
                then.status(200).body("real body");
            })
            .await;

        let client = reqwest::Client::new();
        let request = client
            .get(format!("{}/probe", server.base_url()))
            .build()
            .expect("valid request");

        let response = execute_and_observe(&client, request)
            .await
            .expect("request succeeds");
        mock.assert_async().await;

        // The observer saw the real body...
        assert_eq!(
            *observer
                .responses
                .lock()
                .expect("lock is never held across a panic"),
            vec![(200, b"real body".to_vec())]
        );
        // ...and the caller can still read the very same body from the
        // returned response — the "fork" didn't consume the one copy the
        // caller needs to decode.
        let text = response
            .text()
            .await
            .expect("rebuilt response is still readable");
        assert_eq!(text, "real body");
    }

    #[tokio::test]
    async fn execute_and_observe_metadata_only_withholds_the_body_but_leaves_it_readable() {
        let _guard = TEST_LOCK.lock().await;
        let _clear = ClearObserver;

        let observer = Arc::new(RecordingObserver::default());
        set_transport_observer(Some(observer.clone()));

        let server = httpmock::MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(httpmock::Method::GET).path("/probe");
                then.status(200).body("secret log line");
            })
            .await;

        let client = reqwest::Client::new();
        let request = client
            .get(format!("{}/probe", server.base_url()))
            .build()
            .expect("valid request");

        let response = execute_and_observe_metadata_only(&client, request)
            .await
            .expect("request succeeds");

        assert_eq!(
            *observer
                .responses
                .lock()
                .expect("lock is never held across a panic"),
            vec![(200, WITHHELD_BODY.to_vec())]
        );
        let text = response.text().await.expect("response is still readable");
        assert_eq!(text, "secret log line");
    }

    #[tokio::test]
    async fn execute_and_observe_skips_the_read_and_rebuild_with_no_observer_registered() {
        let _guard = TEST_LOCK.lock().await;
        let _clear = ClearObserver;
        set_transport_observer(None);

        let server = httpmock::MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::GET).path("/probe");
                then.status(200).body("real body");
            })
            .await;

        let client = reqwest::Client::new();
        let request = client
            .get(format!("{}/probe", server.base_url()))
            .build()
            .expect("valid request");

        let response = execute_and_observe(&client, request)
            .await
            .expect("request succeeds");
        mock.assert_async().await;

        let text = response.text().await.expect("response is still readable");
        assert_eq!(text, "real body");
    }

    #[tokio::test]
    async fn no_observer_registered_is_a_silent_no_op() {
        let _guard = TEST_LOCK.lock().await;
        let _clear = ClearObserver;
        set_transport_observer(None);

        let request = reqwest::Request::new(
            reqwest::Method::GET,
            "https://example.test".parse().expect("valid url"),
        );
        notify_request(&request);
    }
}
