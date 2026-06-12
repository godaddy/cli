//! GoDaddy Domains API client (availability + suggest).
//!
//! The contents of this crate are **generated** by `progenitor` at build time
//! from the vendored OpenAPI 3.0 spec (`openapi/domains.oas3.json`). Construct
//! [`Client`] with [`Client::new_with_client`] to supply a pre-authenticated
//! `reqwest::Client` (the CLI sets the `Authorization: sso-key …`/Bearer header
//! itself). See `scripts/regenerate-spec.sh` to refresh the spec.
//!
//! Generated code is exempt from the workspace's strict style lints.
#![allow(clippy::all)]
#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(rustdoc::all)]

include!(concat!(env!("OUT_DIR"), "/codegen.rs"));

/// Error building the authenticated HTTP client.
#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    #[error("invalid header value: {0}")]
    Header(#[from] reqwest::header::InvalidHeaderValue),
    #[error("failed to build HTTP client: {0}")]
    Http(#[from] reqwest::Error),
}

/// Build a [`Client`] whose every request carries a pre-set `Authorization`
/// header and `x-request-id`.
///
/// `authorization` is the full header value the domain endpoints expect — e.g.
/// `"sso-key <KEY>:<SECRET>"` (the usual path) or `"Bearer <token>"`. Keeping
/// the `reqwest::Client` construction here means callers never name reqwest's
/// types, so the main crate is unaffected by this crate's reqwest version.
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
