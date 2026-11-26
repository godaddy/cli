use std::cell::Cell;

use super::*;
use crate::error::codes;
use cli_engine::{CliCoreError, build_error_envelope};
use httpmock::{HttpMockRequest, prelude::*};
use reqwest::Client as ReqwestClient;
use serde_json::{Value, from_slice, json};

const STORE_ID: &str = "6d1f0f4c-8a2e-4a52-9a5b-0f6a3c1d2e4b";
const PRODUCT_ID: &str = "0c3b1a52-77de-4c0e-8f6b-1d2a9e4f5b60";

fn client_for(server: &MockServer) -> CatalogClient {
    CatalogClient::new(
        ReqwestClient::builder(),
        &server.base_url(),
        "tok",
        STORE_ID,
        "ote",
    )
    .expect("build client")
}

fn archived() -> Value {
    json!({
        "id": PRODUCT_ID, "name": "classic-tee", "label": "Classic Tee",
        "status": "ARCHIVED", "type": "PHYSICAL", "description": null,
        "htmlDescription": null, "skuCount": 0,
        "createdAt": "2026-01-01T00:00:00Z", "updatedAt": "2026-01-03T00:00:00Z",
        "activatedAt": null, "archivedAt": "2026-01-03T00:00:00Z",
        "priceRange": null, "compareAtPriceRange": null
    })
}

fn args(product_id: &str, with_variants: bool) -> ArchiveArgs {
    ArchiveArgs {
        store: StoreIdArg { store_id: None },
        product_id: product_id.to_owned(),
        with_variants,
    }
}

fn variable(request: &HttpMockRequest, pointer: &str) -> Option<Value> {
    from_slice::<Value>(request.body_ref())
        .ok()?
        .pointer(&format!("/variables/{pointer}"))
        .cloned()
}

fn code_of(error: &CliCoreError) -> String {
    build_error_envelope(error, "commerce")
        .error
        .map(|e| e.code)
        .unwrap_or_default()
}

async fn run(server: &MockServer, given: &ArchiveArgs) -> Result<CommandResult> {
    archive(given, "ote", false, || {
        let client = client_for(server);
        async move { Ok(client) }
    })
    .await
}

/// Without `--with-variants` no cascade is sent at all, so the variants keep
/// their status and the API's own default is what applies.
#[tokio::test]
async fn by_default_only_the_product_is_archived() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(POST).is_true(|request| {
                variable(request, "id") == Some(json!(PRODUCT_ID))
                    && variable(request, "cascadeArchive").is_none()
            });
            then.status(200)
                .json_body(json!({ "data": { "archiveSkuGroup": archived() } }));
        })
        .await;

    let result = run(&server, &args(PRODUCT_ID, false))
        .await
        .expect("archived");

    mock.assert_async().await;
    assert_eq!(result.data["status"], "ARCHIVED");
    assert_eq!(result.data["archivedAt"], "2026-01-03T00:00:00Z");
}

#[tokio::test]
async fn with_variants_the_variants_are_archived_too() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(POST).is_true(|request| {
                variable(request, "cascadeArchive") == Some(json!({ "skus": true }))
            });
            then.status(200)
                .json_body(json!({ "data": { "archiveSkuGroup": archived() } }));
        })
        .await;

    run(&server, &args(PRODUCT_ID, true))
        .await
        .expect("archived");

    mock.assert_async().await;
}

#[tokio::test]
async fn a_malformed_id_never_reaches_the_api() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(POST);
            then.status(200)
                .json_body(json!({ "data": { "archiveSkuGroup": archived() } }));
        })
        .await;
    let connected = Cell::new(false);

    let error = archive(&args("not-a-uuid", true), "ote", false, || {
        connected.set(true);
        let client = client_for(&server);
        async move { Ok(client) }
    })
    .await
    .expect_err("malformed");

    assert_eq!(code_of(&error), codes::VALIDATION_ERROR);
    assert!(!connected.get(), "the API was connected to");
    assert_eq!(mock.calls_async().await, 0);
}

#[tokio::test]
async fn a_missing_product_is_a_clear_not_found() {
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(POST);
            then.status(200)
                .json_body(json!({ "data": { "archiveSkuGroup": null } }));
        })
        .await;

    let error = run(&server, &args(PRODUCT_ID, false))
        .await
        .expect_err("missing");

    let envelope = build_error_envelope(&error, "commerce");
    assert_eq!(code_of(&error), codes::NOT_FOUND);
    assert!(
        envelope
            .error
            .map(|e| e.message)
            .unwrap_or_default()
            .contains(PRODUCT_ID),
        "the product is named"
    );
}

#[tokio::test]
async fn the_read_back_step_names_the_store_only_when_one_was_given() {
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(POST);
            then.status(200)
                .json_body(json!({ "data": { "archiveSkuGroup": archived() } }));
        })
        .await;
    let mut named = args(PRODUCT_ID, false);
    named.store.store_id = Some(STORE_ID.to_owned());

    let with = run(&server, &named).await.expect("archived");
    let without = run(&server, &args(PRODUCT_ID, false))
        .await
        .expect("archived");

    assert_eq!(
        with.metadata.next_actions[0].params["store-id"]
            .value
            .as_deref(),
        Some(STORE_ID)
    );
    assert!(
        !without.metadata.next_actions[0]
            .command
            .contains("--store-id")
    );
}

#[test]
fn the_command_is_a_mutation_that_declares_the_write_scope() {
    let spec = command().spec;

    assert_eq!(spec.tier, Some(Tier::Mutate));
    assert!(spec.mutates);
    assert_eq!(
        spec.metadata().scopes,
        vec![COMMERCE_PRODUCT_WRITE.to_owned()]
    );
}

/// A preview reads the product, since the real run needs it to be there, and
/// archives nothing: the one request is the read, never the mutation.
#[tokio::test]
async fn a_preview_reads_the_product_and_archives_nothing() {
    let server = MockServer::start_async().await;
    let read = server
        .mock_async(|when, then| {
            when.method(POST)
                .is_true(|request| request.body_ref().windows(8).any(|w| w == b"skuGroup"));
            then.status(200)
                .json_body(json!({ "data": { "skuGroup": archived() } }));
        })
        .await;
    let write = server
        .mock_async(|when, then| {
            when.method(POST).is_true(|request| {
                from_slice::<Value>(request.body_ref())
                    .ok()
                    .and_then(|body| {
                        body["query"]
                            .as_str()
                            .map(|q| q.contains("archiveSkuGroup"))
                    })
                    == Some(true)
            });
            then.status(200)
                .json_body(json!({ "data": { "archiveSkuGroup": archived() } }));
        })
        .await;

    let result = archive(&args(PRODUCT_ID, true), "ote", true, || {
        let client = client_for(&server);
        async move { Ok(client) }
    })
    .await
    .expect("a preview");

    assert!(result.metadata.dry_run, "marked as a preview");
    assert_eq!(write.calls_async().await, 0, "nothing was archived");
    assert_eq!(read.calls_async().await, 1, "the product was read");
    assert_eq!(result.data["action"], "dry-run: would archive product");
    assert_eq!(result.data["productId"], PRODUCT_ID);
    assert_eq!(
        result.data["changes"],
        json!({ "variants": "archived with the product" })
    );
}

#[tokio::test]
async fn a_preview_without_the_cascade_says_nothing_about_variants() {
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(POST);
            then.status(200)
                .json_body(json!({ "data": { "skuGroup": archived() } }));
        })
        .await;

    let result = archive(&args(PRODUCT_ID, false), "ote", true, || {
        let client = client_for(&server);
        async move { Ok(client) }
    })
    .await
    .expect("a preview");

    assert_eq!(result.data["changes"], json!({}));
}

#[tokio::test]
async fn a_preview_of_a_missing_product_is_not_found() {
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(POST);
            then.status(200)
                .json_body(json!({ "data": { "skuGroup": null } }));
        })
        .await;

    let error = archive(&args(PRODUCT_ID, false), "ote", true, || {
        let client = client_for(&server);
        async move { Ok(client) }
    })
    .await
    .expect_err("missing");

    assert_eq!(code_of(&error), codes::NOT_FOUND);
}
