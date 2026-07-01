//! GoDaddy Domains API client, spanning two API generations behind one host:
//!
//! * **v3** — the Domain Lifecycle Management API (`/v3/domains/…`): suggestions,
//!   availability (single + batch), domain get, registration (quote → register),
//!   async operation polling, single DNS-record create, and nameserver replace.
//! * **v1** — the operations v3 does not yet serve (`/v1/domains/…`): list the
//!   shopper's domains, TLD legal agreements, and DNS record list/replace/delete.
//!   Their generated types are `V1`-prefixed to avoid clashing with the v3 ones.
//!
//! The contents of this crate are **generated** by `progenitor` at build time
//! from the vendored, merged OpenAPI 3.0 spec (`openapi/domains.oas3.json`).
//! Construct [`Client`] with [`Client::new_with_client`] to supply a
//! pre-authenticated `reqwest::Client` (the CLI sets the `Authorization:
//! sso-key …`/Bearer header itself). The v3 operations live under the
//! `/v3/domains` base path, baked into the spec's absolute paths so one host
//! `base_url` serves both generations. See `scripts/regenerate-spec.sh` to
//! refresh and re-merge the spec.
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
    // server: HTTP method + path (v3 lives under /v3/domains/…, v1 under
    // /v1/domains/…), the query-parameter / body field names that map the builder
    // setters to the wire, the `Authorization` / `x-request-id` / `Idempotency-Key`
    // headers, and response deserialization. They run entirely offline.

    fn client_for(server: &MockServer) -> Client {
        client_with_auth(
            &server.base_url(),
            "Bearer tok",
            "godaddy-cli/test",
            "req-1",
        )
        .expect("build client")
    }

    // --- v3: discovery ------------------------------------------------------

    #[tokio::test]
    async fn suggest_domains_maps_setters_to_named_query_params() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/v3/domains/suggestions")
                    .query_param("query", "coffee")
                    .query_param("pageSize", "5")
                    .query_param("tlds", "com")
                    .header("authorization", "Bearer tok");
                then.status(200)
                    .json_body(json!({ "items": [{ "domain": "coffeehouse.com" }] }));
            })
            .await;

        let resp = client_for(&server)
            .suggest_domains()
            .query("coffee")
            .page_size(5)
            .tlds(vec!["com".to_string()])
            .send()
            .await
            .expect("request succeeds")
            .into_inner();

        mock.assert_async().await;
        assert_eq!(resp.items.len(), 1);
        assert_eq!(resp.items[0].domain.as_deref(), Some("coffeehouse.com"));
    }

    #[tokio::test]
    async fn get_domain_availability_single_parses_prices() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/v3/domains/check-availability")
                    .query_param("domain", "example.com");
                then.status(200).json_body(json!({
                    "domain": "example.com",
                    "available": true,
                    "definitive": true,
                    // v3 money is ISO-4217 minor units: USD 11.99 -> 1199 (not v1 micro-units).
                    "prices": [{ "period": 1, "price": { "currencyCode": "USD", "value": 1199 } }]
                }));
            })
            .await;

        let body = client_for(&server)
            .get_domain_availability()
            .domain("example.com")
            .send()
            .await
            .expect("request succeeds")
            .into_inner();

        mock.assert_async().await;
        assert_eq!(body.domain.as_deref(), Some("example.com"));
        assert_eq!(body.available, Some(true));
        let prices = body.prices.expect("prices present");
        assert_eq!(prices[0].price.as_ref().and_then(|m| m.value), Some(1199));
    }

    #[tokio::test]
    async fn check_availability_batch_posts_domains_array() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v3/domains/check-availability")
                    .json_body(json!({ "domains": ["a.com", "b.com"], "optimizeFor": "SPEED" }));
                then.status(200).json_body(json!({
                    "items": [
                        { "domain": "a.com", "available": true },
                        { "domain": "b.com", "available": false }
                    ]
                }));
            })
            .await;

        let body = client_for(&server)
            .check_availability()
            .body(types::AvailabilityCheckCriteria {
                domains: vec!["a.com".to_string(), "b.com".to_string()],
                optimize_for: types::OptimizationTarget::Speed,
                isc_code: None,
            })
            .send()
            .await
            .expect("request succeeds")
            .into_inner();

        mock.assert_async().await;
        assert_eq!(body.items.len(), 2);
        assert_eq!(body.items[1].available, Some(false));
    }

    // --- v3: registration (quote → register → poll) -------------------------

    #[tokio::test]
    async fn quote_registration_posts_body_and_parses_token_and_agreements() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v3/domains/registration-quotes")
                    .json_body(json!({ "domain": "example.com", "period": 2 }));
                then.status(200).json_body(json!({
                    "domain": "example.com",
                    "available": true,
                    "quoteToken": "tok-abc",
                    "period": 2,
                    // v3 money is ISO-4217 minor units: USD 23.98 (2yr) -> 2398.
                    "price": { "currencyCode": "USD", "value": 2398 },
                    "requiredAgreements": [
                        { "agreementType": "REGISTRATION", "title": "Registration Agreement",
                          "url": "https://x/agr" }
                    ]
                }));
            })
            .await;

        let quote = client_for(&server)
            .quote_domain_registration()
            .body(types::QuoteDomainRegistrationBody {
                domain: "example.com".to_string(),
                period: std::num::NonZeroU64::new(2).expect("nonzero"),
                profile: None,
                profile_id: None,
            })
            .send()
            .await
            .expect("request succeeds")
            .into_inner();

        mock.assert_async().await;
        assert_eq!(quote.available, Some(true));
        assert_eq!(
            quote.quote_token.as_ref().map(|t| t.as_str()),
            Some("tok-abc")
        );
        let agreements = quote.required_agreements.expect("agreements");
        assert_eq!(
            agreements[0].agreement_type.as_ref().map(|a| a.as_str()),
            Some("REGISTRATION")
        );
    }

    #[tokio::test]
    async fn register_sends_idempotency_key_and_consent_then_accepts_202() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v3/domains/registrations")
                    .header("Idempotency-Key", "idem-123")
                    .json_body(json!({
                        "domain": "example.com",
                        "period": 1,
                        "quoteToken": "tok-abc",
                        "consent": {
                            "agreedAt": "2026-06-30T00:00:00Z",
                            "agreedBy": { "type": "SHOPPER", "principal": "shopper-42", "ip": "127.0.0.1" },
                            "agreementTypes": ["REGISTRATION"]
                        }
                    }));
                // The register response does NOT echo `quoteToken` (the token is
                // single-use and consumed); the client's `Registration` type must
                // parse it anyway — quote_token is optional for exactly this reason.
                then.status(202).json_body(json!({
                    "domain": "example.com",
                    "period": 1,
                    "consent": {
                        "agreedAt": "2026-06-30T00:00:00Z",
                        "agreedBy": { "type": "SHOPPER", "principal": "shopper-42" },
                        "agreementTypes": ["REGISTRATION"]
                    },
                    "registrationId": "reg-1",
                    "operationId": "op-1",
                    "status": "PENDING"
                }));
            })
            .await;

        let reg = client_for(&server)
            .register_domain()
            .idempotency_key("idem-123")
            .body(types::Registration {
                consent: types::Consent {
                    agreed_at: types::DateTime("2026-06-30T00:00:00Z".to_string()),
                    agreed_by: types::ConsentActor {
                        actor: None,
                        ip: Some("127.0.0.1".to_string()),
                        principal: "shopper-42".to_string(),
                        type_: types::ConsentActorType("SHOPPER".to_string()),
                    },
                    agreement_types: vec![types::AgreementType("REGISTRATION".to_string())],
                },
                created_at: None,
                domain: "example.com".to_string(),
                expires_at: None,
                links: vec![],
                operation_id: None,
                period: std::num::NonZeroU64::new(1).expect("nonzero"),
                profile: None,
                profile_id: None,
                quote_token: Some(types::Uuid("tok-abc".to_string())),
                registration_id: None,
                status: None,
                updated_at: None,
            })
            .send()
            .await
            .expect("202 accepted")
            .into_inner();

        mock.assert_async().await;
        assert_eq!(reg.operation_id.as_ref().map(|o| o.as_str()), Some("op-1"));
    }

    #[tokio::test]
    async fn get_operation_polls_status() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(GET).path("/v3/domains/operations/op-1");
                then.status(200).json_body(json!({
                    "operationId": "op-1",
                    "type": "REGISTER",
                    "domain": "example.com",
                    "status": "COMPLETED"
                }));
            })
            .await;

        let op = client_for(&server)
            .get_operation()
            .operation_id(types::Uuid("op-1".to_string()))
            .send()
            .await
            .expect("request succeeds")
            .into_inner();

        mock.assert_async().await;
        assert_eq!(op.status.as_ref().map(|s| s.as_str()), Some("COMPLETED"));
    }

    // --- v3: domain get + nameservers + dns create --------------------------

    #[tokio::test]
    async fn get_domain_reads_v3_path() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/v3/domains/domain-names/example.com");
                then.status(200).json_body(json!({
                    "domain": "example.com",
                    "status": "ACTIVE",
                    "autoRenew": true
                }));
            })
            .await;

        let detail = client_for(&server)
            .get_domain()
            .domain_name("example.com")
            .send()
            .await
            .expect("request succeeds")
            .into_inner();

        mock.assert_async().await;
        assert_eq!(detail.domain.as_deref(), Some("example.com"));
        assert_eq!(detail.auto_renew, Some(true));
    }

    #[tokio::test]
    async fn create_dns_record_posts_single_record_to_zone() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v3/domains/zones/example.com/dns-records")
                    .json_body(
                        json!({ "type": "A", "name": "www", "data": "1.2.3.4", "ttl": 600 }),
                    );
                then.status(201).json_body(
                    json!({ "type": "A", "name": "www", "data": "1.2.3.4", "ttl": 600 }),
                );
            })
            .await;

        let rec = client_for(&server)
            .create_dns_record()
            .zone("example.com")
            .body(types::DnsRecord {
                data: "1.2.3.4".to_string(),
                flag: None,
                name: "www".to_string(),
                port: None,
                priority: None,
                protocol: None,
                record_id: None,
                service: None,
                tag: None,
                ttl: 600,
                type_: types::DnsRecordType("A".to_string()),
                weight: None,
            })
            .send()
            .await
            .expect("request succeeds")
            .into_inner();

        mock.assert_async().await;
        assert_eq!(rec.name, "www");
        assert_eq!(rec.ttl, 600);
    }

    #[tokio::test]
    async fn update_nameservers_puts_hostname_array() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(PUT)
                    .path("/v3/domains/domain-names/example.com/nameservers")
                    .header("Idempotency-Key", "idem-9")
                    .json_body(json!(["ns1.example.net", "ns2.example.net"]));
                then.status(202)
                    .json_body(json!({ "operationId": "op-2", "type": "UPDATE_NAMESERVERS" }));
            })
            .await;

        let op = client_for(&server)
            .update_nameservers()
            .domain_name("example.com")
            .idempotency_key("idem-9")
            .body(types::NameServers(vec![
                types::NameserverHostname("ns1.example.net".to_string()),
                types::NameserverHostname("ns2.example.net".to_string()),
            ]))
            .send()
            .await
            .expect("202 accepted")
            .into_inner();

        mock.assert_async().await;
        assert_eq!(op.operation_id.as_ref().map(|o| o.as_str()), Some("op-2"));
    }

    // --- retained v1: list + agreements + DNS list/set/delete ---------------

    #[tokio::test]
    async fn v1_list_tolerates_sparse_payloads() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(GET).path("/v1/domains");
                then.status(200).json_body(json!([
                    { "domain": "a.com", "status": "ACTIVE", "nameServers": null },
                    { "domain": "b.me", "status": "PENDING_DNS_ACTIVE", "nameServers": null }
                ]));
            })
            .await;

        let body = client_for(&server)
            .list()
            .send()
            .await
            .expect("sparse list parses")
            .into_inner();

        mock.assert_async().await;
        assert_eq!(body.len(), 2);
        assert_eq!(body[0].domain.as_deref(), Some("a.com"));
    }

    #[tokio::test]
    async fn v1_agreements_sends_query_params_and_parses_list() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/v1/domains/agreements")
                    .query_param("tlds", "com")
                    .query_param("privacy", "false");
                then.status(200).json_body(json!([
                    { "agreementKey": "DNRA", "title": "Registration Agreement", "url": "https://x" }
                ]));
            })
            .await;

        let agreements = client_for(&server)
            .agreements()
            .tlds(vec!["com".to_string()])
            .privacy(false)
            .send()
            .await
            .expect("request succeeds")
            .into_inner();

        mock.assert_async().await;
        assert_eq!(agreements[0].agreement_key.as_deref(), Some("DNRA"));
    }

    #[tokio::test]
    async fn v1_record_get_all_lists_records() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(GET).path("/v1/domains/example.com/records");
                then.status(200).json_body(
                    json!([{ "type": "A", "name": "www", "data": "1.2.3.4", "ttl": 600 }]),
                );
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
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].data.as_deref(), Some("1.2.3.4"));
    }

    #[tokio::test]
    async fn v1_record_replace_type_name_puts_record_set() {
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
            .body(vec![types::V1dnsRecordCreateTypeName {
                data: "5.6.7.8".to_string(),
                port: None,
                priority: None,
                protocol: None,
                service: None,
                ttl: Some(600),
                weight: None,
            }])
            .send()
            .await
            .expect("request succeeds");

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn v1_record_delete_type_name_issues_delete() {
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

    // Guard the auth-scheme selection retained from the hand-written helper.
    #[tokio::test]
    async fn sso_key_scheme_sets_authorization_header() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/v3/domains/check-availability")
                    .header("authorization", "sso-key KEY:SECRET");
                then.status(200)
                    .json_body(json!({ "domain": "x.com", "available": true }));
            })
            .await;

        client_with_auth(
            &server.base_url(),
            "sso-key KEY:SECRET",
            "godaddy-cli/test",
            "req-1",
        )
        .expect("build client")
        .get_domain_availability()
        .domain("x.com")
        .send()
        .await
        .expect("request succeeds");

        mock.assert_async().await;
    }
}
