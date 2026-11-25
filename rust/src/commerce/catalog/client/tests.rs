use std::sync::{Arc, Mutex};

use super::*;
use crate::error::codes;
use cli_engine::build_error_envelope;
use generated_client_support::{TransportObserver, set_transport_observer};
use httpmock::{HttpMockRequest, prelude::*};
use reqwest::{Client as ReqwestClient, Request, StatusCode, header::HeaderMap};
use serde_json::{from_slice, json};

const STORE_ID: &str = "6d1f0f4c-8a2e-4a52-9a5b-0f6a3c1d2e4b";

fn client_for(server: &MockServer, store_id: &str) -> CatalogClient {
    CatalogClient::new(
        ReqwestClient::builder(),
        &server.base_url(),
        "tok",
        store_id,
        "ote",
    )
    .expect("build client")
}

fn text_of(error: &CliCoreError) -> String {
    let envelope = build_error_envelope(error, "commerce");
    format!(
        "{} {}",
        envelope.error.map(|e| e.message).unwrap_or_default(),
        envelope.fix.unwrap_or_default()
    )
}

fn code_of(error: &CliCoreError) -> String {
    build_error_envelope(error, "commerce")
        .error
        .map(|e| e.code)
        .unwrap_or_default()
}

/// A matcher for a request whose `variables` are exactly `expected`, so a member
/// that should have been left out fails the match instead of being ignored.
fn variables_are(expected: Value) -> impl Fn(&HttpMockRequest) -> bool + Send + Sync + 'static {
    move |request| {
        from_slice::<Value>(request.body_ref())
            .is_ok_and(|body| body.get("variables") == Some(&expected))
    }
}

fn empty_page() -> Value {
    json!({ "data": { "skuGroups": {
        "totalCount": 0,
        "pageInfo": { "hasNextPage": false, "endCursor": null },
        "edges": []
    } } })
}

async fn serve(server: &MockServer, when: impl FnOnce(httpmock::When) -> httpmock::When) {
    server
        .mock_async(|w, then| {
            when(w.method(POST));
            then.status(200).json_body(empty_page());
        })
        .await;
}

#[tokio::test]
async fn a_request_goes_to_the_store_scoped_subgraph_with_its_headers() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(POST)
                .path(format!("/v2/commerce/stores/{STORE_ID}/catalog-subgraph"))
                .header("authorization", "Bearer tok")
                .header("x-store-id", STORE_ID)
                .header_exists("x-request-id");
            then.status(200).json_body(empty_page());
        })
        .await;

    client_for(&server, STORE_ID)
        .list_products(10, None, None)
        .await
        .expect("the request succeeds");

    mock.assert_async().await;
}

/// The subgraph refuses `null` for a variable that was not set, so the request
/// must not carry one. This pins the exact serialized variables.
#[tokio::test]
async fn unset_variables_are_absent_from_the_request() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(POST)
                .is_true(variables_are(json!({ "first": 10 })));
            then.status(200).json_body(empty_page());
        })
        .await;

    client_for(&server, STORE_ID)
        .list_products(10, None, None)
        .await
        .expect("the request succeeds");

    mock.assert_async().await;
}

#[tokio::test]
async fn set_variables_are_present_in_the_request() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(POST).is_true(variables_are(json!({
                "first": 25,
                "after": "cursor-1",
                "status": { "in": ["ACTIVE", "DRAFT"] }
            })));
            then.status(200).json_body(empty_page());
        })
        .await;
    let statuses = vec!["ACTIVE".to_owned(), "DRAFT".to_owned()];

    client_for(&server, STORE_ID)
        .list_products(25, Some("cursor-1"), Some(&statuses))
        .await
        .expect("the request succeeds");

    mock.assert_async().await;
}

/// An unset member inside a filter that is set is also refused when it is
/// `null`, so only the member that was set is sent.
#[tokio::test]
async fn an_unset_member_of_a_set_filter_is_absent_too() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(POST).is_true(move |request| {
                from_slice::<Value>(request.body_ref())
                    .ok()
                    .and_then(|body| body.pointer("/variables/status").cloned())
                    == Some(json!({ "in": ["ACTIVE"] }))
            });
            then.status(200).json_body(empty_page());
        })
        .await;

    client_for(&server, STORE_ID)
        .list_products(10, None, Some(&["ACTIVE".to_owned()]))
        .await
        .expect("the request succeeds");

    mock.assert_async().await;
}

#[test]
fn only_unset_members_are_removed_never_deliberate_values() {
    let mut variables = json!({
        "unset": null,
        "keep_false": false,
        "keep_zero": 0,
        "keep_empty": "",
        "keep_empty_list": [],
        "nested": { "unset": null, "keep": "x", "deeper": { "unset": null, "keep": 1 } },
        "list": [null, { "unset": null, "keep": true }, "y"]
    });

    omit_unset(&mut variables);

    assert_eq!(
        variables,
        json!({
            "keep_false": false,
            "keep_zero": 0,
            "keep_empty": "",
            "keep_empty_list": [],
            "nested": { "keep": "x", "deeper": { "keep": 1 } },
            "list": [null, { "keep": true }, "y"]
        })
    );
}

#[tokio::test]
async fn a_page_of_products_is_returned_with_its_cursor() {
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(POST);
            then.status(200).json_body(json!({ "data": { "skuGroups": {
                "totalCount": 1,
                "pageInfo": { "hasNextPage": true, "endCursor": "next-1" },
                "edges": [{ "node": {
                    "id": "p-1", "name": "tee", "label": "Tee", "status": "ACTIVE",
                    "type": "PHYSICAL", "description": null, "skuCount": 3,
                    "createdAt": "2026-01-01T00:00:00Z", "updatedAt": "2026-01-02T00:00:00Z",
                    "priceRange": null, "compareAtPriceRange": null
                } }]
            } } }));
        })
        .await;

    let page = client_for(&server, STORE_ID)
        .list_products(10, None, None)
        .await
        .expect("the request succeeds")
        .expect("the store has a products connection");

    assert!(page.page_info.has_next_page);
    assert_eq!(page.page_info.end_cursor.as_deref(), Some("next-1"));
    let edges = page.edges.expect("edges");
    let node = edges[0]
        .as_ref()
        .expect("an edge")
        .node
        .as_ref()
        .expect("a node");
    assert_eq!(node.id, "p-1");
}

#[tokio::test]
async fn a_store_with_no_products_connection_is_none_not_an_error() {
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(POST);
            then.status(200)
                .json_body(json!({ "data": { "skuGroups": null } }));
        })
        .await;

    let page = client_for(&server, STORE_ID)
        .list_products(10, None, None)
        .await
        .expect("the request succeeds");

    assert!(page.is_none());
}

/// A GraphQL error is a failure even on HTTP 200, and even beside partial data.
#[tokio::test]
async fn a_graphql_error_on_http_200_is_a_failure() {
    for body in [
        json!({ "errors": [{ "message": "not authorized" }] }),
        json!({
            "data": { "skuGroups": null },
            "errors": [{ "message": "not authorized" }]
        }),
    ] {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(POST);
                then.status(200).json_body(body.clone());
            })
            .await;

        let error = client_for(&server, STORE_ID)
            .list_products(10, None, None)
            .await
            .expect_err("an errors array is a failure");

        assert!(text_of(&error).contains("not authorized"), "{body}");
        assert_eq!(code_of(&error), codes::NETWORK_ERROR);
    }
}

/// The subgraph's refusal of a malformed request names the fields, in a message
/// that is mostly serialized validator output. The error describes the fields.
#[tokio::test]
async fn a_validation_error_names_the_fields_the_api_refused() {
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(POST);
            then.status(200).json_body(json!({ "errors": [{
                "message": "[{ \"code\": \"invalid_union\", \"unionErrors\": [] }]",
                "extensions": {
                    "code": "VALIDATION_ERROR",
                    "fields": [{ "code": "invalid_union", "path": ["status"] }]
                }
            }] }));
        })
        .await;

    let error = client_for(&server, STORE_ID)
        .list_products(10, None, None)
        .await
        .expect_err("a refused request is a failure");

    let text = text_of(&error);
    assert!(text.contains("status (invalid_union)"), "{text}");
    assert!(!text.contains("unionErrors"), "{text}");
    assert_eq!(code_of(&error), codes::VALIDATION_ERROR);
}

#[tokio::test]
async fn a_response_with_neither_data_nor_errors_is_a_failure() {
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(POST);
            then.status(200).json_body(json!({}));
        })
        .await;

    let error = client_for(&server, STORE_ID)
        .list_products(10, None, None)
        .await
        .expect_err("no data is a failure");

    assert_eq!(code_of(&error), codes::UNEXPECTED_ERROR);
}

#[tokio::test]
async fn an_http_error_status_is_a_coded_failure_that_keeps_its_status() {
    for (status, expected_code) in [
        (404, codes::NOT_FOUND),
        (403, codes::NETWORK_ERROR),
        (401, codes::AUTH_REQUIRED),
        (503, codes::NETWORK_ERROR),
    ] {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(POST);
                then.status(status).body("upstream unavailable");
            })
            .await;

        let error = client_for(&server, STORE_ID)
            .list_products(10, None, None)
            .await
            .expect_err("an error status is a failure");

        assert_eq!(code_of(&error), expected_code, "{status}");
        let text = text_of(&error);
        assert!(!text.contains("upstream unavailable"), "{status}: {text}");
    }
}

#[tokio::test]
async fn a_404_names_the_store_and_the_environment() {
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(POST);
            then.status(404);
        })
        .await;

    let error = client_for(&server, STORE_ID)
        .list_products(10, None, None)
        .await
        .expect_err("a missing store is a failure");

    let text = text_of(&error);
    assert!(text.contains(STORE_ID), "{text}");
    assert!(text.contains("ote"), "{text}");
}

#[tokio::test]
async fn an_unparseable_body_is_a_failure_that_does_not_quote_it() {
    let secret = "SECRET-GOV-ID-123-45-6789";
    for body in [
        format!("<html>{secret}</html>"),
        json!({ "data": { "skuGroups": { "totalCount": secret } } }).to_string(),
    ] {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(POST);
                then.status(200).body(body.clone());
            })
            .await;

        let error = client_for(&server, STORE_ID)
            .list_products(10, None, None)
            .await
            .expect_err("an unreadable body is a failure");

        assert_eq!(code_of(&error), codes::UNEXPECTED_ERROR, "{body}");
        let text = format!("{:?}", build_error_envelope(&error, "commerce"));
        assert!(!text.contains(secret), "{body}: {text}");
    }
}

/// Nothing a response says outside its `errors` array may reach an error: a
/// catalog record can carry data a merchant would not want in a bug report.
#[tokio::test]
async fn error_text_never_contains_data_from_the_response() {
    let secret = "SECRET-GOV-ID-123-45-6789";
    for (status, body) in [
        (500, format!("upstream said: {secret}")),
        (422, json!({ "data": { "leak": secret } }).to_string()),
    ] {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(POST);
                then.status(status).body(body.clone());
            })
            .await;

        let error = client_for(&server, STORE_ID)
            .list_products(10, None, None)
            .await
            .expect_err("each of these is a failure");

        let text = format!("{:?}", build_error_envelope(&error, "commerce"));
        assert!(!text.contains(secret), "{status}: {text}");
    }
}

#[derive(Default)]
struct Recorder {
    requests: Mutex<Vec<String>>,
    responses: Mutex<Vec<Vec<u8>>>,
}

impl TransportObserver for Recorder {
    fn on_request(&self, request: &Request) {
        self.requests
            .lock()
            .expect("lock is never held across a panic")
            .push(request.url().path().to_owned());
    }

    fn on_response(&self, _status: StatusCode, _headers: &HeaderMap, body: &[u8]) {
        self.responses
            .lock()
            .expect("lock is never held across a panic")
            .push(body.to_vec());
    }
}

/// Requests go through the shared transport observer, so `--debug transport`
/// sees them. The observer is process-wide and other tests also send requests,
/// so this one looks only at its own store id.
#[tokio::test]
async fn requests_reach_the_transport_observer() {
    let own_store = "0c3b1a52-77de-4c0e-8f6b-1d2a9e4f5b60";
    let recorder = Arc::new(Recorder::default());
    set_transport_observer(Some(recorder.clone()));
    let server = MockServer::start_async().await;
    serve(&server, |when| when).await;

    client_for(&server, own_store)
        .list_products(10, None, None)
        .await
        .expect("the request succeeds");
    set_transport_observer(None);

    let requests = recorder
        .requests
        .lock()
        .expect("lock is never held across a panic");
    assert!(
        requests.iter().any(|path| path.contains(own_store)),
        "{requests:?}"
    );
    assert!(
        !recorder
            .responses
            .lock()
            .expect("lock is never held across a panic")
            .is_empty()
    );
}

#[test]
fn a_cleartext_non_loopback_base_url_is_refused() {
    let result = CatalogClient::new(
        ReqwestClient::builder(),
        "http://api.example.com",
        "tok",
        STORE_ID,
        "ote",
    );

    let error = result.err().expect("cleartext is refused");
    assert_eq!(code_of(&error), codes::CONFIG_ERROR);
}

/// A store id is put in the path, so one with a `/` or `?` must not change its
/// structure.
#[tokio::test]
async fn the_store_id_is_encoded_in_the_path() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/v2/commerce/stores/a%2Fb%3Fc/catalog-subgraph");
            then.status(200).json_body(empty_page());
        })
        .await;

    client_for(&server, "a/b?c")
        .list_products(10, None, None)
        .await
        .expect("the request succeeds");

    mock.assert_async().await;
}
