use super::*;
use crate::{
    environments::test_support::{ENV_LOCK, EnvGuard},
    error::codes,
};
use cli_engine::{Envelope, build_error_envelope};
use httpmock::prelude::*;
use reqwest::Client as HttpClient;
use serde_json::json;

const STORE_ID: &str = "6d1f0f4c-8a2e-4a52-9a5b-0f6a3c1d2e4b";

fn client_for(server: &MockServer) -> Client {
    client_with_builder(
        HttpClient::builder(),
        &server.base_url(),
        "Bearer tok",
        "req-1",
    )
    .expect("build client")
}

fn envelope(error: &CliCoreError) -> Envelope {
    build_error_envelope(error, "commerce")
}

fn code(error: &CliCoreError) -> String {
    envelope(error).error.map(|e| e.code).unwrap_or_default()
}

#[tokio::test]
async fn a_store_is_read_through_the_v1_path_with_auth_headers() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(GET)
                .path(format!("/v1/commerce/stores/{STORE_ID}"))
                .header("authorization", "Bearer tok");
            then.status(200).json_body(json!({
                "id": STORE_ID,
                "businessId": "biz-1",
                "displayName": "Corner Shop",
                "status": "ACTIVE"
            }));
        })
        .await;

    let store = fetch_store(&client_for(&server), STORE_ID, "ote")
        .await
        .expect("the store is returned");

    mock.assert_async().await;
    assert_eq!(store.display_name.as_deref(), Some("Corner Shop"));
    assert_eq!(store.business_id.as_deref(), Some("biz-1"));
}

/// A missing store says which store and environment, and sends the user to where
/// store ids are listed, rather than the generic "HTTP error 404".
#[tokio::test]
async fn a_404_names_the_store_and_says_where_to_find_one() {
    let _lock = ENV_LOCK.lock().await;
    let _unset = EnvGuard::unset("GDDY_DEVELOPER_URL");
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(GET)
                .path(format!("/v1/commerce/stores/{STORE_ID}"));
            then.status(404);
        })
        .await;

    let error = fetch_store(&client_for(&server), STORE_ID, "ote")
        .await
        .expect_err("a missing store is an error");

    let envelope = envelope(&error);
    let message = envelope.error.as_ref().map(|e| e.message.clone());
    let message = message.unwrap_or_default();
    assert!(message.contains(STORE_ID), "{message}");
    assert!(message.contains("ote"), "{message}");
    assert_eq!(code(&error), codes::NOT_FOUND);
    let fix = envelope.fix.unwrap_or_default();
    assert!(fix.contains("Your stores"), "{fix}");
    assert!(fix.contains("developer.ote-godaddy.com"), "{fix}");
}

/// The 404 text must not leak progenitor's placeholder URL for an unexpected
/// response (see DEVEX-1390).
#[tokio::test]
async fn a_404_does_not_show_the_placeholder_request_url() {
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(GET);
            then.status(404);
        })
        .await;

    let error = fetch_store(&client_for(&server), STORE_ID, "ote")
        .await
        .expect_err("a missing store is an error");

    let rendered = format!("{:?}", envelope(&error));
    assert!(!rendered.contains("no.url.provided"), "{rendered}");
}

#[tokio::test]
async fn a_403_says_the_account_may_not_read_the_store() {
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(GET)
                .path(format!("/v1/commerce/stores/{STORE_ID}"));
            then.status(403).body("forbidden");
        })
        .await;

    let error = fetch_store(&client_for(&server), STORE_ID, "ote")
        .await
        .expect_err("a forbidden store is an error");

    let envelope = envelope(&error);
    let message = envelope
        .error
        .as_ref()
        .map(|e| e.message.clone())
        .unwrap_or_default();
    assert!(message.contains(STORE_ID), "{message}");
    assert!(message.contains("not allowed"), "{message}");
    // A forbidden store is not a missing one: callers branch on the code.
    assert_ne!(code(&error), codes::NOT_FOUND);
    assert_eq!(code(&error), codes::NETWORK_ERROR);
    assert!(
        envelope
            .fix
            .as_deref()
            .is_some_and(|fix| fix.contains("gddy auth scope")),
        "{envelope:?}"
    );
}

#[tokio::test]
async fn a_server_error_keeps_its_status() {
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(GET);
            then.status(503).body("unavailable");
        })
        .await;

    let error = fetch_store(&client_for(&server), STORE_ID, "ote")
        .await
        .expect_err("a 503 is an error");

    let message = envelope(&error)
        .error
        .map(|e| e.message)
        .unwrap_or_default();
    assert!(message.contains("503"), "{message}");
}

#[tokio::test]
async fn a_store_response_that_cannot_be_read_is_reported_not_swallowed() {
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(GET);
            then.status(200).body("not json");
        })
        .await;

    let error = fetch_store(&client_for(&server), STORE_ID, "ote")
        .await
        .expect_err("an unreadable body is an error");

    assert_eq!(code(&error), codes::UNEXPECTED_ERROR);
}
