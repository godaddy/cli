//! Generated GoDaddy Business Email Management API client
//! (`checkMailboxEligibility`, `getMailbox`, `createMailbox`, `listMailboxes`).
//!
//! The contents of this crate are **generated** by `progenitor` at build time
//! from the vendored OpenAPI 3.0 spec (`openapi/email.oas3.json`). Construct
//! [`Client`] with [`Client::new_with_client`] to supply a pre-authenticated
//! `reqwest::Client` (the CLI sets the `Authorization: Bearer <token>` header
//! itself) or use [`client_with_auth`]. The spec's paths are baked in as
//! `/v1/email/...` absolute paths, so callers pass a bare host `base_url`.
//!
//! The lint allowances are scoped to the generated module so the hand-written
//! code below (`client_with_auth`, the `ClientHooks` bridge) is still linted
//! normally. `BuildError`/`TransportObserver`/`set_transport_observer` are
//! shared with every other generated client crate — see
//! `generated-client-support`.

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
pub use generated_client_support::{BuildError, TransportObserver, set_transport_observer};

/// Bridges generated requests/responses into the registered
/// [`TransportObserver`], if any.
///
/// progenitor generates every call as `client.pre(...)`, `client.exec(...)`,
/// `client.post(...)` (see [`progenitor_client::ClientHooks`]); the default
/// impl (for `&Client`) is a no-op. Implementing the trait for `Client`
/// (without the reference) overrides it via progenitor's "auto-ref
/// specialization" — this is the sanctioned extension point, not a hack.
/// This impl has to live here (Rust's orphan rule: it's a foreign trait for
/// this crate's own `Client` type) even though the observer plumbing itself
/// is shared — see `generated_client_support`.
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

/// Build a [`Client`] whose every request carries a pre-set `Authorization`
/// header and `x-request-id`.
///
/// `authorization` is the full header value the email endpoints expect — e.g.
/// `"Bearer <token>"`. Keeping the `reqwest::Client` construction here means
/// callers never name reqwest's types, so the main crate is unaffected by this
/// crate's reqwest version.
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

    // These tests exercise the generated request/response wiring against a mock
    // server: HTTP method + path (under /v1/email/…), the query-parameter / body
    // field names that map the builder setters to the wire, the `Authorization` /
    // `x-request-id` / `Idempotency-Key` headers, and response deserialization.
    // They run entirely offline.

    fn client_for(server: &MockServer) -> Client {
        client_with_auth(
            &server.base_url(),
            "Bearer tok",
            "godaddy-cli/test",
            "req-1",
        )
        .expect("build client")
    }

    #[tokio::test]
    async fn check_mailbox_eligibility_sends_email_query_param() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/v1/email/check-mailbox-eligibility")
                    .query_param("email", "jane@example.com")
                    .header("authorization", "Bearer tok");
                then.status(200).json_body(json!({ "isEligible": true }));
            })
            .await;

        let result = client_for(&server)
            .check_mailbox_eligibility()
            .email("jane@example.com")
            .send()
            .await
            .expect("request succeeds")
            .into_inner();

        mock.assert_async().await;
        assert!(result.is_eligible);
    }

    #[tokio::test]
    async fn get_mailbox_reads_by_id() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(GET).path("/v1/email/mailboxes/mbx-456");
                then.status(200).json_body(json!({
                    "mailboxId": "mbx-456",
                    "emailAddress": "jane@example.com",
                    "status": "COMPLETED"
                }));
            })
            .await;

        let mailbox = client_for(&server)
            .get_mailbox()
            .mailbox_id("mbx-456")
            .send()
            .await
            .expect("request succeeds")
            .into_inner();

        mock.assert_async().await;
        assert_eq!(
            mailbox.mailbox_id.map(|id| id.to_string()),
            Some("mbx-456".to_owned())
        );
        assert_eq!(
            mailbox.status.map(|s| s.to_string()),
            Some("COMPLETED".to_owned())
        );
    }

    #[tokio::test]
    async fn list_mailboxes_sends_paging_query_params() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/v1/email/mailboxes")
                    .query_param("page", "2")
                    .query_param("pageSize", "10");
                then.status(200).json_body(json!({ "items": [] }));
            })
            .await;

        let list = client_for(&server)
            .list_mailboxes()
            .page(2u64)
            .page_size(10u64)
            .send()
            .await
            .expect("request succeeds")
            .into_inner();

        mock.assert_async().await;
        assert!(list.items.is_empty());
    }

    #[tokio::test]
    async fn create_mailbox_sends_idempotency_key_and_body() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v1/email/mailboxes")
                    .header_exists("idempotency-key")
                    .json_body(json!({ "emailAddress": "jane@example.com" }));
                then.status(202).json_body(json!({
                    "mailboxId": "mbx-456",
                    "emailAddress": "jane@example.com",
                    "status": "EXECUTING"
                }));
            })
            .await;

        let created = client_for(&server)
            .create_mailbox()
            .idempotency_key("idem-key-0123456789")
            .body(types::CreateMailboxBody {
                account_id: None,
                consents: vec![],
                created_at: None,
                display_name: None,
                email_address: "jane@example.com".to_owned(),
                first_name: None,
                last_name: None,
                links: vec![],
                mailbox_id: None,
                mailbox_type: None,
                status: None,
                updated_at: None,
            })
            .send()
            .await
            .expect("202 accepted")
            .into_inner();

        mock.assert_async().await;
        assert_eq!(
            created.mailbox_id.map(|id| id.to_string()),
            Some("mbx-456".to_owned())
        );
    }

    // --- ClientHooks / TransportObserver bridge ------------------------------

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

    #[tokio::test]
    async fn client_hooks_feed_request_and_response_events_to_the_registered_observer() {
        let _test_lock = TRANSPORT_OBSERVER_TEST_LOCK.lock().await;
        let _clear = ClearTransportObserver;

        let observer = std::sync::Arc::new(RecordingObserver::default());
        set_transport_observer(Some(observer.clone()));

        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(GET).path("/v1/email/mailboxes");
                then.status(200).json_body(json!({ "items": [] }));
            })
            .await;

        let list = client_for(&server)
            .list_mailboxes()
            .send()
            .await
            .expect("request succeeds")
            .into_inner();
        mock.assert_async().await;

        assert!(list.items.is_empty());

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
            responses
                .iter()
                .any(|(status, body)| *status == 200
                    && String::from_utf8_lossy(body).contains("items")),
            "expected a response event carrying the real body, got: {responses:?}"
        );
    }
}
