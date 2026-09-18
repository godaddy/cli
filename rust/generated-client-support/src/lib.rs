//! Shared hand-written support code for `progenitor`-generated API client
//! crates (`domains-client`, `shopping-client`, `email-client`, ...).
//!
//! `progenitor` generates each crate's `Client` type itself, so the pieces
//! that have to name that type — `Client::new_with_client`, and the
//! `progenitor_client::ClientHooks<()> for Client` impl Rust's orphan rule
//! requires to live alongside `Client` — can't move here. Everything else
//! that every generated client crate re-derives identically lives here
//! instead: the authenticated-`reqwest::Client` builder, and the
//! [`TransportObserver`] extension point + its process-wide registry.

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
}

/// Builds the pre-authenticated `reqwest::Client` a generated client's
/// `client_with_auth` wraps in `Client::new_with_client(base_url, ..)`.
/// `authorization` is the full header value the API expects, e.g.
/// `"Bearer <token>"`.
pub fn build_authenticated_http_client(
    authorization: &str,
    user_agent: &str,
    request_id: &str,
) -> Result<reqwest::Client, BuildError> {
    let mut headers = HeaderMap::new();
    headers.insert(AUTHORIZATION, HeaderValue::from_str(authorization)?);
    headers.insert(
        HeaderName::from_static("x-request-id"),
        HeaderValue::from_str(request_id)?,
    );
    Ok(reqwest::Client::builder()
        .user_agent(user_agent)
        .default_headers(headers)
        .build()?)
}

/// Observes every request/response a generated client makes, independent of
/// any particular logging backend. This crate has no compile-time dependency
/// on `cli-engine` (or any other framework) — the *caller* pushes an
/// implementation in via [`set_transport_observer`] rather than this crate
/// pulling one in.
pub trait TransportObserver: Send + Sync {
    fn on_request(&self, request: &reqwest::Request);
    fn on_response(&self, status: reqwest::StatusCode, headers: &reqwest::header::HeaderMap);
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

/// Forwards a generated request to the registered [`TransportObserver`], if
/// any. Called from each generated client crate's
/// `progenitor_client::ClientHooks::pre` — that impl has to live in the
/// crate that defines `Client` (orphan rule), so this is the one-line body
/// it delegates to.
pub fn notify_request(request: &reqwest::Request) {
    let observer = TRANSPORT_OBSERVER
        .read()
        .expect("lock is never held across a panic")
        .clone();
    if let Some(observer) = observer {
        observer.on_request(request);
    }
}

/// Forwards a generated response to the registered [`TransportObserver`], if
/// any — the counterpart to [`notify_request`], called from
/// `ClientHooks::post`. Silently does nothing for an `Err` (a communication
/// failure never reached a response to observe).
pub fn notify_response_result(result: &reqwest::Result<reqwest::Response>) {
    let observer = TRANSPORT_OBSERVER
        .read()
        .expect("lock is never held across a panic")
        .clone();
    if let Ok(response) = result
        && let Some(observer) = observer
    {
        observer.on_response(response.status(), response.headers());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_authenticated_http_client_rejects_an_invalid_header_value() {
        let err = build_authenticated_http_client("Bearer tok\n", "test-agent", "req-1")
            .expect_err("a header value containing a newline is invalid");
        assert!(matches!(err, BuildError::Header(_)));
    }

    #[test]
    fn build_authenticated_http_client_succeeds_with_valid_inputs() {
        build_authenticated_http_client("Bearer tok", "test-agent", "req-1")
            .expect("valid inputs build a client");
    }

    #[derive(Debug, Default)]
    struct RecordingObserver {
        requests: std::sync::Mutex<Vec<String>>,
        responses: std::sync::Mutex<Vec<u16>>,
    }

    impl TransportObserver for RecordingObserver {
        fn on_request(&self, request: &reqwest::Request) {
            self.requests
                .lock()
                .expect("lock is never held across a panic")
                .push(request.method().to_string());
        }

        fn on_response(&self, status: reqwest::StatusCode, _headers: &reqwest::header::HeaderMap) {
            self.responses
                .lock()
                .expect("lock is never held across a panic")
                .push(status.as_u16());
        }
    }

    // Serializes tests that mutate the process-wide observer. An async-aware
    // lock isn't needed here (no `.await` while held), but a plain
    // `std::sync::Mutex` still keeps the tests in this module from
    // interleaving their observer mutation.
    static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    struct ClearObserver;
    impl Drop for ClearObserver {
        fn drop(&mut self) {
            set_transport_observer(None);
        }
    }

    #[test]
    fn notify_request_reaches_the_registered_observer() {
        let _guard = TEST_LOCK.lock().expect("lock is never held across a panic");
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

    // `notify_response_result`'s `Ok` path is covered end-to-end by each
    // generated client crate's own `client_hooks_feed_the_registered_observer`
    // test against a real mock response — no `Err(reqwest::Error)` can be
    // constructed outside the `reqwest` crate to test the other branch here.

    #[test]
    fn no_observer_registered_is_a_silent_no_op() {
        let _guard = TEST_LOCK.lock().expect("lock is never held across a panic");
        let _clear = ClearObserver;
        set_transport_observer(None);

        let request = reqwest::Request::new(
            reqwest::Method::GET,
            "https://example.test".parse().expect("valid url"),
        );
        notify_request(&request);
    }
}
