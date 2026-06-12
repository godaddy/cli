//! GoDaddy Domains API client (availability + suggest).
//!
//! The contents of this crate are **generated** by `progenitor` at build time
//! from the vendored OpenAPI 3.0 spec (`openapi/domains.oas3.json`). Construct
//! [`Client`] with [`Client::new_with_client`] to supply a pre-authenticated
//! `reqwest::Client` (the CLI sets the `Authorization: sso-key …`/Bearer header
//! itself). See `scripts/regenerate-spec.sh` to refresh the spec.
//!
//! The lint allowances are scoped to the generated module so the hand-written
//! code below (`client_with_auth`, `BuildError`) is still linted normally.

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

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;
    use serde_json::json;

    // These tests exercise the generated request/response wiring against a mock
    // server: the query-parameter names (which guard the builder setters →
    // wire-parameter mapping at the call sites), the `Authorization`/
    // `x-request-id`/`api-version` headers set by `client_with_auth`, and response
    // deserialization. They run entirely offline.

    #[tokio::test]
    async fn available_sends_correct_request_and_parses_response() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/v1/domains/available")
                    .query_param("domain", "example.com")
                    .query_param("checkType", "FULL")
                    .query_param("forTransfer", "true")
                    .header("authorization", "sso-key KEY:SECRET")
                    .header("x-request-id", "req-123")
                    .header("api-version", "1.0.0");
                then.status(200).json_body(json!({
                    "domain": "example.com",
                    "available": false,
                    "definitive": true,
                    "price": 11_990_000,
                    "currency": "USD",
                    "renewalPrice": 21_990_000,
                    "period": 1
                }));
            })
            .await;

        let client = client_with_auth(
            &server.base_url(),
            "sso-key KEY:SECRET",
            "godaddy-cli/test",
            "req-123",
        )
        .expect("build client");

        let body = client
            .available()
            .domain("example.com")
            .check_type(types::AvailableCheckType::Full)
            .for_transfer(true)
            .send()
            .await
            .expect("request succeeds")
            .into_inner();

        mock.assert_async().await;
        assert_eq!(body.domain, "example.com");
        assert!(!body.available);
        assert!(body.definitive);
        assert_eq!(body.price, Some(11_990_000));
        assert_eq!(body.currency, "USD");
        assert_eq!(body.renewal_price, Some(21_990_000));
        assert_eq!(body.period, Some(1));
    }

    #[tokio::test]
    async fn available_with_bearer_scheme_sets_header() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/v1/domains/available")
                    .query_param("domain", "open.dev")
                    .header("authorization", "Bearer tok-abc");
                then.status(200).json_body(json!({
                    "domain": "open.dev",
                    "available": true,
                    "definitive": true
                }));
            })
            .await;

        let client = client_with_auth(
            &server.base_url(),
            "Bearer tok-abc",
            "godaddy-cli/test",
            "req-1",
        )
        .expect("build client");

        let body = client
            .available()
            .domain("open.dev")
            .send()
            .await
            .expect("request succeeds")
            .into_inner();

        mock.assert_async().await;
        assert!(body.available);
        // Optional fields absent in the response deserialize to None.
        assert_eq!(body.price, None);
        assert_eq!(body.currency, "USD"); // serde default
    }

    #[tokio::test]
    async fn suggest_maps_positional_args_to_named_query_params() {
        let server = MockServer::start_async().await;
        // Asserting each value lands in the correctly *named* query param guards
        // the builder setter -> wire-parameter mapping (e.g. that `.city(..)`
        // really sends `city=`, not some other param) across spec regenerations.
        let mock = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/v1/domains/suggest")
                    .query_param("query", "coffee")
                    .query_param("city", "Phoenix")
                    .query_param("country", "US")
                    .query_param("limit", "5")
                    .query_param("tlds", "com");
                then.status(200).json_body(json!([
                    { "domain": "coffeehouse.com" },
                    { "domain": "bestcoffee.com" }
                ]));
            })
            .await;

        let client = client_with_auth(
            &server.base_url(),
            "Bearer tok",
            "godaddy-cli/test",
            "req-2",
        )
        .expect("build client");

        let suggestions = client
            .suggest()
            .query("coffee")
            .city("Phoenix")
            .country(types::SuggestCountry::Us)
            .limit(5)
            .tlds(vec!["com".to_string()])
            .send()
            .await
            .expect("request succeeds")
            .into_inner();

        mock.assert_async().await;
        let domains: Vec<&str> = suggestions.iter().map(|s| s.domain.as_str()).collect();
        assert_eq!(domains, ["coffeehouse.com", "bestcoffee.com"]);
    }
}
