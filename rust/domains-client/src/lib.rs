//! GoDaddy Domains API client, spanning two API generations behind one host:
//!
//! * **v3** — the Domain Lifecycle Management API (`/v3/domains/…`): suggestions,
//!   availability (single + batch), domain get, registration (quote → register),
//!   async operation polling, the full DNS record lifecycle (create / list /
//!   replace / delete), and nameserver replace.
//! * **v1** — the operations v3 does not yet serve (`/v1/domains/…`): list the
//!   shopper's domains and TLD legal agreements. Their generated types are
//!   `V1`-prefixed to avoid clashing with the v3 ones.
//!
//! The contents of this crate are **generated** by `progenitor` at build time
//! from the vendored, merged OpenAPI 3.0 spec (`openapi/domains.oas3.json`).
//! Construct [`Client`] with [`Client::new_with_client`] to supply a
//! pre-authenticated `reqwest::Client` (the CLI sets the `Authorization:
//! Bearer <token>` header itself). The v3 operations live under the
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

/// Observes every generated request/response, independent of any particular
/// logging backend. This crate has no compile-time dependency on cli-engine
/// (or any other framework) — the *caller* pushes an implementation in via
/// [`set_transport_observer`] rather than this crate pulling one in.
pub trait TransportObserver: Send + Sync {
    fn on_request(&self, request: &reqwest::Request);
    fn on_response(&self, status: reqwest::StatusCode, headers: &reqwest::header::HeaderMap);
}

static TRANSPORT_OBSERVER: std::sync::RwLock<Option<std::sync::Arc<dyn TransportObserver>>> =
    std::sync::RwLock::new(None);

/// Registers (or clears, with `None`) the process-wide transport observer.
/// The main crate calls this with an adapter around its own logging
/// framework — e.g. cli-engine's `--debug transport` bridge — before making
/// any request through a [`Client`].
pub fn set_transport_observer(observer: Option<std::sync::Arc<dyn TransportObserver>>) {
    *TRANSPORT_OBSERVER
        .write()
        .expect("lock is never held across a panic") = observer;
}

/// Bridges generated requests/responses into the registered
/// [`TransportObserver`], if any.
///
/// progenitor generates every call as `client.pre(...)`, `client.exec(...)`,
/// `client.post(...)` (see [`progenitor_client::ClientHooks`]); the default
/// impl (for `&Client`) is a no-op. Implementing the trait for `Client`
/// (without the reference) overrides it via progenitor's "auto-ref
/// specialization" — this is the sanctioned extension point, not a hack.
///
/// `post` only gets `&reqwest::Result<Response>` (a reference, pre-body-read),
/// so response bodies aren't capturable here without consuming the body ahead
/// of the generated code's own deserialization — this reports status/headers
/// only for responses; request bodies are reported in full via `pre`.
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

/// Build a [`Client`] whose every request carries a pre-set `Authorization`
/// header and `x-request-id`.
///
/// `authorization` is the full header value the domain endpoints expect — e.g.
/// `"Bearer <token>"`. Keeping the `reqwest::Client` construction here means
/// callers never name reqwest's types, so the main crate is unaffected by this
/// crate's reqwest version.
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
                        { "agreementType": "API_DPA", "title": "Registration Agreement",
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
            agreements[0].agreement_type.as_ref().map(|a| a.to_string()),
            Some("API_DPA".to_string())
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
                            "agreedBy": { "type": "DIRECT", "principal": "shopper-42", "ip": "127.0.0.1" },
                            "agreementTypes": ["API_DPA"]
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
                        "agreedBy": { "type": "DIRECT", "principal": "shopper-42" },
                        "agreementTypes": ["API_DPA"]
                    },
                    "registrationId": "reg-1",
                    "operationId": "op-1",
                    "status": "EXECUTING"
                }));
            })
            .await;

        let reg = client_for(&server)
            .register_domain()
            .idempotency_key("idem-123")
            .body(types::Registration {
                consent: types::Consent {
                    agreed_at: types::DateTime("2026-06-30T00:00:00Z".to_string()),
                    // Sending a client-side ConsentActor is still valid on the
                    // wire (the field is optional, not rejected) even though the
                    // server derives its own `agreedBy` from the auth context.
                    agreed_by: Some(types::ConsentActor {
                        actor: None,
                        ip: Some("127.0.0.1".to_string()),
                        principal: "shopper-42".to_string(),
                        type_: types::ConsentActorType::Direct,
                    }),
                    agreement_types: vec![types::AgreementType::ApiDpa],
                    acknowledged_fees: vec![],
                },
                created_at: None,
                domain: "example.com".to_string(),
                expires_at: None,
                fees: vec![],
                links: vec![],
                operation_id: None,
                order_id: None,
                period: std::num::NonZeroU64::new(1).expect("nonzero"),
                price: None,
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
        assert_eq!(
            op.status.as_ref().map(|s| s.to_string()),
            Some("COMPLETED".to_string())
        );
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
                // `DomainOperationType` is spec'd as `enum: [REGISTER]` only, even
                // though this endpoint's response is a `DomainOperation` too and a
                // real nameserver-update operation would carry a different `type`
                // (e.g. "UPDATE_NAMESERVERS") — a gap in the vendored spec, not this
                // client. "REGISTER" is the only value that currently deserializes.
                then.status(202)
                    .json_body(json!({ "operationId": "op-2", "type": "REGISTER" }));
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

    // --- retained v1: list + agreements -------------------------------------

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

    // --- v3: DNS list / delete -----------------------------------------------

    #[tokio::test]
    async fn list_dns_records_sends_filters_and_parses_collection() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/v3/domains/zones/example.com/dns-records")
                    .query_param("type", "A")
                    .query_param("name", "www")
                    .query_param("page", "1")
                    .query_param("pageSize", "100");
                then.status(200).json_body(json!({
                    "items": [
                        { "type": "A", "name": "www", "data": "1.2.3.4", "ttl": 600, "recordId": "rec-1" }
                    ],
                    "totalItems": 1,
                    "totalPages": 1,
                    "links": []
                }));
            })
            .await;

        let body = client_for(&server)
            .list_dns_records()
            .zone("example.com")
            .type_(types::DnsRecordType("A".to_string()))
            .name("www")
            .page(std::num::NonZeroU64::new(1).expect("nonzero"))
            .page_size(std::num::NonZeroU64::new(100).expect("nonzero"))
            .send()
            .await
            .expect("request succeeds")
            .into_inner();

        mock.assert_async().await;
        let items = body.items.expect("items present");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].record_id.as_deref(), Some("rec-1"));
        assert_eq!(items[0].data, "1.2.3.4");
    }

    #[tokio::test]
    async fn delete_dns_record_deletes_by_record_id() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(DELETE)
                    .path("/v3/domains/zones/example.com/dns-records/rec-1");
                then.status(204);
            })
            .await;

        client_for(&server)
            .delete_dns_record()
            .zone("example.com")
            .record_id("rec-1")
            .send()
            .await
            .expect("request succeeds");

        mock.assert_async().await;
    }

    // --- ClientHooks / TransportObserver bridge ------------------------------

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

    // Serializes tests that mutate the process-wide transport observer. An
    // async-aware lock, not a `std::sync::Mutex` — the guard is held across
    // this test's `.await` points (clippy::await_holding_lock), which is only
    // sound with a lock that yields the executor instead of blocking a thread.
    static TRANSPORT_OBSERVER_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    // Clears the observer on drop so a panicking assertion below can't leak
    // a test's observer into later tests in this binary. Declared after
    // acquiring `TRANSPORT_OBSERVER_TEST_LOCK` so the reset runs while the
    // lock is still held.
    struct ClearTransportObserver;

    impl Drop for ClearTransportObserver {
        fn drop(&mut self) {
            set_transport_observer(None);
        }
    }

    // Generated calls thread every request/response through `ClientHooks`,
    // which this crate implements (see above) to forward to whatever
    // `TransportObserver` the caller has registered — the extension point
    // that lets the main crate bridge to its own `--debug transport` logging
    // without this crate depending on that framework. Assert the bridge
    // actually fires, not just that the request/response round-trips.
    #[tokio::test]
    async fn client_hooks_feed_the_registered_observer() {
        let _test_lock = TRANSPORT_OBSERVER_TEST_LOCK.lock().await;
        let _clear = ClearTransportObserver;

        let observer = std::sync::Arc::new(RecordingObserver::default());
        set_transport_observer(Some(observer.clone()));

        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(GET).path("/v3/domains/suggestions");
                then.status(200).json_body(json!({ "items": [] }));
            })
            .await;

        client_for(&server)
            .suggest_domains()
            .query("coffee")
            .send()
            .await
            .expect("request succeeds");
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
            responses.contains(&200),
            "expected a response event, got: {responses:?}"
        );
    }
}
