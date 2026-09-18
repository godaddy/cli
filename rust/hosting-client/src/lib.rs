//! Typed client generated from the vendored Hosting OpenAPI contract.

mod generated {
    #![allow(clippy::all)]
    #![allow(dead_code)]
    #![allow(unused_imports)]
    #![allow(rustdoc::all)]

    include!(concat!(env!("OUT_DIR"), "/codegen.rs"));
}

pub use generated::*;

#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    #[error("invalid header value: {0}")]
    Header(#[from] reqwest::header::InvalidHeaderValue),
    #[error("failed to build HTTP client: {0}")]
    Http(#[from] reqwest::Error),
}

/// Observes generated requests/responses without this crate depending on
/// cli-engine. The CLI registers an adapter via [`set_transport_observer`].
pub trait TransportObserver: Send + Sync {
    fn on_request(&self, request: &reqwest::Request);
    fn on_response(&self, status: reqwest::StatusCode, headers: &reqwest::header::HeaderMap);
}

static TRANSPORT_OBSERVER: std::sync::RwLock<Option<std::sync::Arc<dyn TransportObserver>>> =
    std::sync::RwLock::new(None);

pub fn set_transport_observer(observer: Option<std::sync::Arc<dyn TransportObserver>>) {
    *TRANSPORT_OBSERVER
        .write()
        .expect("lock is never held across a panic") = observer;
}

impl progenitor_client::ClientHooks<()> for Client {
    async fn pre<E>(
        &self,
        request: &mut reqwest::Request,
        _info: &progenitor_client::OperationInfo,
    ) -> Result<(), progenitor_client::Error<E>> {
        let observer = TRANSPORT_OBSERVER
            .read()
            .expect("lock is never held across a panic")
            .clone();
        if let Some(observer) = observer {
            observer.on_request(request);
        }
        Ok(())
    }

    async fn post<E>(
        &self,
        result: &reqwest::Result<reqwest::Response>,
        _info: &progenitor_client::OperationInfo,
    ) -> Result<(), progenitor_client::Error<E>> {
        let observer = TRANSPORT_OBSERVER
            .read()
            .expect("lock is never held across a panic")
            .clone();
        if let Ok(response) = result
            && let Some(observer) = observer
        {
            observer.on_response(response.status(), response.headers());
        }
        Ok(())
    }
}

pub fn client_with_auth(
    base_url: &str,
    authorization: &str,
    user_agent: &str,
    request_id: &str,
) -> Result<Client, BuildError> {
    use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderName, HeaderValue};

    let mut headers = HeaderMap::new();
    headers.insert(AUTHORIZATION, HeaderValue::from_str(authorization)?);
    headers.insert(
        HeaderName::from_static("x-request-id"),
        HeaderValue::from_str(request_id)?,
    );
    let http = reqwest::Client::builder()
        .user_agent(user_agent)
        .default_headers(headers)
        .build()?;
    Ok(Client::new_with_client(base_url, http))
}
