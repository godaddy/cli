use std::cell::Cell;

use super::*;
use crate::error::{GddyError, codes};
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

fn updated() -> Value {
    json!({
        "id": PRODUCT_ID, "name": "classic-tee", "label": "Classic T-Shirt",
        "status": "ACTIVE", "type": "PHYSICAL", "description": "Soft",
        "htmlDescription": null, "skuCount": 1,
        "createdAt": "2026-01-01T00:00:00Z", "updatedAt": "2026-01-02T00:00:00Z",
        "activatedAt": "2026-01-02T00:00:00Z", "archivedAt": null,
        "priceRange": { "min": 9.5, "max": 9.5 }, "compareAtPriceRange": null
    })
}

fn args(product_id: &str) -> UpdateArgs {
    UpdateArgs {
        store: StoreIdArg { store_id: None },
        product_id: product_id.to_owned(),
        label: None,
        description: None,
        status: None,
        name: None,
    }
}

fn variable(request: &HttpMockRequest, pointer: &str) -> Option<Value> {
    from_slice::<Value>(request.body_ref())
        .ok()?
        .pointer(&format!("/variables/{pointer}"))
        .cloned()
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

async fn run(server: &MockServer, given: &UpdateArgs) -> Result<CommandResult> {
    update(given, "ote", false, || {
        let client = client_for(server);
        async move { Ok(client) }
    })
    .await
}

/// Only the details named are sent, with the product's id as its own variable, so
/// everything else on the product is left as it is.
#[tokio::test]
async fn only_the_details_named_are_sent() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(POST).is_true(|request| {
                variable(request, "id") == Some(json!(PRODUCT_ID))
                    && variable(request, "input") == Some(json!({ "label": "Classic T-Shirt" }))
            });
            then.status(200)
                .json_body(json!({ "data": { "updateSkuGroup": updated() } }));
        })
        .await;
    let mut given = args(PRODUCT_ID);
    given.label = Some("Classic T-Shirt".to_owned());

    let result = run(&server, &given).await.expect("updated");

    mock.assert_async().await;
    assert_eq!(result.data["label"], "Classic T-Shirt");
    assert_eq!(result.data["priceMin"], 9.5);
}

#[tokio::test]
async fn every_detail_named_is_sent() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(POST).is_true(|request| {
                variable(request, "input")
                    == Some(json!({
                        "label": "L", "description": "D", "status": "ACTIVE", "name": "n-1"
                    }))
            });
            then.status(200)
                .json_body(json!({ "data": { "updateSkuGroup": updated() } }));
        })
        .await;
    let mut given = args(PRODUCT_ID);
    given.label = Some("L".to_owned());
    given.description = Some("D".to_owned());
    given.status = Some("ACTIVE".to_owned());
    given.name = Some("n-1".to_owned());

    run(&server, &given).await.expect("updated");

    mock.assert_async().await;
}

/// A product id in another case, or padded, is sent in the form the API takes.
#[tokio::test]
async fn the_id_is_sent_hyphenated_and_in_lower_case() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(POST)
                .is_true(|request| variable(request, "id") == Some(json!(PRODUCT_ID)));
            then.status(200)
                .json_body(json!({ "data": { "updateSkuGroup": updated() } }));
        })
        .await;
    let mut given = args(&format!("  {}  ", PRODUCT_ID.to_uppercase()));
    given.label = Some("L".to_owned());

    run(&server, &given).await.expect("updated");

    mock.assert_async().await;
}

/// An update that names nothing to change is refused with what to give, and
/// reaches neither the API nor a sign-in.
#[tokio::test]
async fn naming_nothing_to_change_is_refused_before_the_api_is_reached() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(POST);
            then.status(200)
                .json_body(json!({ "data": { "updateSkuGroup": updated() } }));
        })
        .await;
    let connected = Cell::new(false);

    let error = update(&args(PRODUCT_ID), "ote", false, || {
        connected.set(true);
        let client = client_for(&server);
        async move { Ok(client) }
    })
    .await
    .expect_err("nothing to change");

    let text = text_of(&error);
    assert_eq!(code_of(&error), codes::VALIDATION_ERROR);
    assert!(text.contains("--label"), "{text}");
    assert!(!connected.get(), "the API was connected to");
    assert_eq!(mock.calls_async().await, 0);
}

#[tokio::test]
async fn a_malformed_id_never_reaches_the_api() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(POST);
            then.status(200)
                .json_body(json!({ "data": { "updateSkuGroup": updated() } }));
        })
        .await;
    let connected = Cell::new(false);
    let mut given = args("not-a-uuid");
    given.label = Some("L".to_owned());

    let error = update(&given, "ote", false, || {
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

/// A product the store does not have is a not-found error that names it, and
/// nothing is reported as changed.
#[tokio::test]
async fn a_missing_product_is_a_clear_not_found() {
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(POST);
            then.status(200)
                .json_body(json!({ "data": { "updateSkuGroup": null } }));
        })
        .await;
    let mut given = args(PRODUCT_ID);
    given.label = Some("L".to_owned());

    let error = run(&server, &given).await.expect_err("missing");

    let text = text_of(&error);
    assert_eq!(code_of(&error), codes::NOT_FOUND);
    assert!(text.contains(PRODUCT_ID), "{text}");
    assert!(text.contains("product list"), "{text}");
}

#[tokio::test]
async fn a_refused_detail_is_named_and_the_api_text_is_not_shown() {
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(POST);
            then.status(200).json_body(json!({ "errors": [{
                "message": "name \"Bad Name!\" is not valid",
                "extensions": { "code": "VALIDATION_ERROR", "fields": [
                    { "code": "invalid_string", "path": ["input", "name"] }
                ] }
            }], "data": { "updateSkuGroup": null } }));
        })
        .await;
    let mut given = args(PRODUCT_ID);
    given.name = Some("Bad Name!".to_owned());

    let error = run(&server, &given).await.expect_err("refused");

    let text = text_of(&error);
    assert_eq!(code_of(&error), codes::VALIDATION_ERROR);
    assert!(text.contains("input.name (invalid_string)"), "{text}");
    assert!(!text.contains("Bad Name!"), "{text}");
}

#[tokio::test]
async fn the_read_back_step_names_the_store_only_when_one_was_given() {
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(POST);
            then.status(200)
                .json_body(json!({ "data": { "updateSkuGroup": updated() } }));
        })
        .await;
    let mut named = args(PRODUCT_ID);
    named.label = Some("L".to_owned());
    named.store.store_id = Some(STORE_ID.to_owned());
    let mut selected = named.clone();
    selected.store.store_id = None;

    let with = run(&server, &named).await.expect("updated");
    let without = run(&server, &selected).await.expect("updated");

    let action = &with.metadata.next_actions[0];
    assert_eq!(action.params["store-id"].value.as_deref(), Some(STORE_ID));
    assert_eq!(
        action.params["product-id"].value.as_deref(),
        Some(PRODUCT_ID)
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
/// changes nothing: the one request is the read, never the mutation.
#[tokio::test]
async fn a_preview_reads_the_product_and_changes_nothing() {
    let server = MockServer::start_async().await;
    let read = server
        .mock_async(|when, then| {
            when.method(POST)
                .is_true(|request| request.body_ref().windows(8).any(|w| w == b"skuGroup"));
            then.status(200)
                .json_body(json!({ "data": { "skuGroup": updated() } }));
        })
        .await;
    let write = server
        .mock_async(|when, then| {
            when.method(POST).is_true(|request| {
                from_slice::<Value>(request.body_ref())
                    .ok()
                    .and_then(|body| body["query"].as_str().map(|q| q.contains("updateSkuGroup")))
                    == Some(true)
            });
            then.status(200)
                .json_body(json!({ "data": { "updateSkuGroup": updated() } }));
        })
        .await;
    let mut given = args(PRODUCT_ID);
    given.label = Some("New".to_owned());
    given.status = Some("ACTIVE".to_owned());

    let result = update(&given, "ote", true, || {
        let client = client_for(&server);
        async move { Ok(client) }
    })
    .await
    .expect("a preview");

    assert!(result.metadata.dry_run, "marked as a preview");
    assert_eq!(write.calls_async().await, 0, "nothing was changed");
    assert_eq!(read.calls_async().await, 1, "the product was read");
    assert_eq!(result.data["action"], "dry-run: would update product");
    assert_eq!(result.data["productId"], PRODUCT_ID);
    assert_eq!(
        result.data["changes"],
        json!({ "label": "New", "status": "ACTIVE" })
    );
}

/// A preview of a change to a product the store does not have fails as the real
/// run would, and not as a change that would succeed.
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
    let mut given = args(PRODUCT_ID);
    given.label = Some("New".to_owned());

    let error = update(&given, "ote", true, || {
        let client = client_for(&server);
        async move { Ok(client) }
    })
    .await
    .expect_err("missing");

    assert_eq!(code_of(&error), codes::NOT_FOUND);
}

/// An empty change is refused in a preview as it is for real, before connecting.
#[tokio::test]
async fn a_preview_with_nothing_to_change_is_refused_before_connecting() {
    let connected = Cell::new(false);

    let error = update(&args(PRODUCT_ID), "ote", true, || {
        connected.set(true);
        async { Err(GddyError::unexpected("not reached").into_cli_error()) }
    })
    .await
    .expect_err("nothing to change");

    assert_eq!(code_of(&error), codes::VALIDATION_ERROR);
    assert!(!connected.get());
}
