//! GoDaddy Domains API client (domains list + availability + suggest + DNS records).
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
    use httpmock::Method::PATCH; // not re-exported by the prelude (unlike GET/PUT/DELETE)
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

    #[tokio::test]
    async fn list_tolerates_sparse_payloads() {
        // The published spec marks fields like `contactRegistrant`/`renewDeadline`
        // required and types `nameServers` as a non-null array, but the live API
        // omits the former and returns `nameServers: null` for many domains
        // (cancelled/pending). The generated `DomainSummary` must read these
        // without erroring. Payload mirrors a real `GET /v1/domains` response.
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(GET).path("/v1/domains");
                then.status(200).json_body(json!([
                    {
                        "createdAt": "2021-09-24T15:08:06.000Z",
                        "deletedAt": "2024-11-05T02:30:31.000Z",
                        "domain": "blahblahblah253.com",
                        "domainId": 21605119,
                        "expirationProtected": true,
                        "expires": "2024-09-24T15:08:06.000Z",
                        "exposeWhois": false,
                        "holdRegistrar": false,
                        "locked": true,
                        "nameServers": null,
                        "privacy": true,
                        "renewAuto": false,
                        "renewable": false,
                        "status": "CANCELLED",
                        "transferProtected": true
                    },
                    {
                        "createdAt": "2020-10-27T13:40:15.463Z",
                        "domain": "dullreferenceexception.me",
                        "domainId": 21507912,
                        "expirationProtected": false,
                        "exposeWhois": false,
                        "holdRegistrar": false,
                        "locked": false,
                        "nameServers": null,
                        "privacy": false,
                        "renewAuto": false,
                        "renewable": false,
                        "status": "PENDING_DNS_ACTIVE",
                        "transferProtected": false
                    }
                ]));
            })
            .await;

        let body = client_for(&server)
            .list()
            .send()
            .await
            .expect("sparse list payload parses")
            .into_inner();

        mock.assert_async().await;
        assert_eq!(body.len(), 2);
    }

    // --- DNS records ---------------------------------------------------------
    //
    // These guard the spec-generated record operations: the HTTP method + path
    // (including the `{domain}`/`{type}`/`{name}` path segments and the
    // synthesized list-all GET), the JSON request bodies the builder serializes,
    // and response parsing. They run entirely offline against a mock server.

    fn client_for(server: &MockServer) -> Client {
        client_with_auth(
            &server.base_url(),
            "Bearer tok",
            "godaddy-cli/test",
            "req-rec",
        )
        .expect("build client")
    }

    #[tokio::test]
    async fn record_get_all_lists_every_record() {
        // No type/name -> the synthesized GET on the bare `/records` path.
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/v1/domains/example.com/records")
                    .header("authorization", "Bearer tok");
                then.status(200).json_body(json!([
                    { "type": "A", "name": "www", "data": "1.2.3.4", "ttl": 600 },
                    { "type": "TXT", "name": "@", "data": "v=spf1 -all" }
                ]));
            })
            .await;

        let records = client_for(&server)
            .record_get_all()
            .domain("example.com")
            .send()
            .await
            .expect("request succeeds")
            .into_inner();

        mock.assert_async().await;
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].type_, types::DnsRecordType::A);
        assert_eq!(records[0].name, "www");
        assert_eq!(records[0].data, "1.2.3.4");
        assert_eq!(records[0].ttl, Some(600));
        assert_eq!(records[1].type_, types::DnsRecordType::Txt);
    }

    #[tokio::test]
    async fn record_get_sends_type_name_and_pagination() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/v1/domains/example.com/records/A/www")
                    .query_param("limit", "10")
                    .query_param("offset", "5");
                then.status(200)
                    .json_body(json!([{ "type": "A", "name": "www", "data": "1.2.3.4" }]));
            })
            .await;

        let records = client_for(&server)
            .record_get()
            .domain("example.com")
            .type_("A")
            .name("www")
            .limit(10)
            .offset(5)
            .send()
            .await
            .expect("request succeeds")
            .into_inner();

        mock.assert_async().await;
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].data, "1.2.3.4");
    }

    #[tokio::test]
    async fn record_add_patches_a_record_array() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(PATCH)
                    .path("/v1/domains/example.com/records")
                    .json_body(json!([{ "data": "1.2.3.4", "name": "www", "type": "A" }]));
                then.status(200);
            })
            .await;

        client_for(&server)
            .record_add()
            .domain("example.com")
            .body(vec![types::DnsRecord {
                data: "1.2.3.4".to_string(),
                name: "www".to_string(),
                type_: types::DnsRecordType::A,
                ttl: None,
                priority: None,
                port: None,
                weight: None,
                protocol: None,
                service: None,
            }])
            .send()
            .await
            .expect("request succeeds");

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn record_replace_type_name_puts_the_record_set() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(PUT)
                    .path("/v1/domains/example.com/records/A/www")
                    .json_body(json!([{ "data": "5.6.7.8", "ttl": 600 }]));
                then.status(200);
            })
            .await;

        client_for(&server)
            .record_replace_type_name()
            .domain("example.com")
            .type_("A")
            .name("www")
            .body(vec![types::DnsRecordCreateTypeName {
                data: "5.6.7.8".to_string(),
                ttl: Some(600),
                priority: None,
                port: None,
                weight: None,
                protocol: None,
                service: None,
            }])
            .send()
            .await
            .expect("request succeeds");

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn record_delete_type_name_issues_delete_and_accepts_204() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(DELETE)
                    .path("/v1/domains/example.com/records/A/www");
                then.status(204);
            })
            .await;

        client_for(&server)
            .record_delete_type_name()
            .domain("example.com")
            .type_("A")
            .name("www")
            .send()
            .await
            .expect("request succeeds");

        mock.assert_async().await;
    }
}
