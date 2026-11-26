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

fn created() -> Value {
    json!({
        "id": PRODUCT_ID, "name": "classic-tee", "label": "Classic Tee",
        "status": "DRAFT", "type": "PHYSICAL", "description": null,
        "htmlDescription": null, "skuCount": 0,
        "createdAt": "2026-01-01T00:00:00Z", "updatedAt": "2026-01-01T00:00:00Z",
        "activatedAt": null, "archivedAt": null,
        "priceRange": null, "compareAtPriceRange": null
    })
}

fn args(store_id: Option<&str>) -> CreateArgs {
    CreateArgs {
        store: StoreIdArg {
            store_id: store_id.map(str::to_owned),
        },
        label: "Classic Tee".to_owned(),
        kind: "PHYSICAL".to_owned(),
        description: None,
        status: None,
        name: None,
    }
}

fn input_sent(request: &HttpMockRequest) -> Option<Value> {
    from_slice::<Value>(request.body_ref())
        .ok()?
        .pointer("/variables/input")
        .cloned()
}

fn code_of(error: &CliCoreError) -> String {
    build_error_envelope(error, "commerce")
        .error
        .map(|e| e.code)
        .unwrap_or_default()
}

async fn serve(server: &MockServer, body: Value) -> httpmock::Mock<'_> {
    server
        .mock_async(move |when, then| {
            when.method(POST);
            then.status(200).json_body(body.clone());
        })
        .await
}

/// Only what the person gave is sent, so the API's own defaults apply to the
/// rest. Nothing unset is sent as `null`, which the API refuses.
#[tokio::test]
async fn only_the_details_given_are_sent() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(POST).is_true(|request| {
                input_sent(request) == Some(json!({ "label": "Classic Tee", "type": "PHYSICAL" }))
            });
            then.status(200)
                .json_body(json!({ "data": { "createSkuGroup": created() } }));
        })
        .await;

    create(&args(None), false, || {
        let client = client_for(&server);
        async move { Ok(client) }
    })
    .await
    .expect("created");

    mock.assert_async().await;
}

#[tokio::test]
async fn every_detail_given_is_sent() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(POST).is_true(|request| {
                input_sent(request)
                    == Some(json!({
                        "label": "Classic Tee", "type": "DIGITAL", "description": "Soft",
                        "status": "ACTIVE", "name": "classic-tee"
                    }))
            });
            then.status(200)
                .json_body(json!({ "data": { "createSkuGroup": created() } }));
        })
        .await;
    let mut given = args(None);
    given.kind = "DIGITAL".to_owned();
    given.description = Some("Soft".to_owned());
    given.status = Some("ACTIVE".to_owned());
    given.name = Some("classic-tee".to_owned());

    create(&given, false, || {
        let client = client_for(&server);
        async move { Ok(client) }
    })
    .await
    .expect("created");

    mock.assert_async().await;
}

/// The result is the product as the API stored it, in the same terms `get`
/// shows, and the next step reads it back from the same store.
#[tokio::test]
async fn the_result_is_the_new_product_and_a_step_to_read_it_back() {
    let server = MockServer::start_async().await;
    serve(&server, json!({ "data": { "createSkuGroup": created() } })).await;

    let result = create(&args(Some(STORE_ID)), false, || {
        let client = client_for(&server);
        async move { Ok(client) }
    })
    .await
    .expect("created");

    assert_eq!(result.data["id"], PRODUCT_ID);
    assert_eq!(result.data["label"], "Classic Tee");
    assert_eq!(result.data["status"], "DRAFT");
    assert_eq!(result.data["type"], "PHYSICAL");
    assert!(result.data.get("skuCount").is_none(), "{}", result.data);
    let action = &result.metadata.next_actions[0];
    assert!(
        action
            .command
            .ends_with("commerce catalog product get <product-id> --store-id <store-id>"),
        "{}",
        action.command
    );
    assert_eq!(
        action.params["product-id"].value.as_deref(),
        Some(PRODUCT_ID)
    );
    assert_eq!(action.params["store-id"].value.as_deref(), Some(STORE_ID));
}

#[tokio::test]
async fn the_read_back_step_names_no_store_when_none_was_given() {
    let server = MockServer::start_async().await;
    serve(&server, json!({ "data": { "createSkuGroup": created() } })).await;

    let result = create(&args(None), false, || {
        let client = client_for(&server);
        async move { Ok(client) }
    })
    .await
    .expect("created");

    let action = &result.metadata.next_actions[0];
    assert!(!action.command.contains("--store-id"), "{}", action.command);
}

/// A refusal of the request names the fields the API found wrong, so the person
/// knows what to change, and not the API's free text, which can quote values.
#[tokio::test]
async fn a_refused_request_says_which_detail_was_wrong() {
    let server = MockServer::start_async().await;
    serve(
        &server,
        json!({ "errors": [{
            "message": "name \"Bad Name!\" is not valid",
            "extensions": { "code": "VALIDATION_ERROR", "fields": [
                { "code": "invalid_string", "path": ["input", "name"] }
            ] }
        }], "data": { "createSkuGroup": null } }),
    )
    .await;
    let mut given = args(None);
    given.name = Some("Bad Name!".to_owned());

    let error = create(&given, false, || {
        let client = client_for(&server);
        async move { Ok(client) }
    })
    .await
    .expect_err("refused");

    let text = format!("{:?}", build_error_envelope(&error, "commerce"));
    assert_eq!(code_of(&error), codes::VALIDATION_ERROR);
    assert!(text.contains("input.name (invalid_string)"), "{text}");
    assert!(
        !text.contains("Bad Name!"),
        "the API's text was shown: {text}"
    );
}

/// An answer that holds no product is not a created product.
#[tokio::test]
async fn an_answer_with_no_product_is_not_a_success() {
    let server = MockServer::start_async().await;
    serve(&server, json!({ "data": { "createSkuGroup": null } })).await;

    let error = create(&args(None), false, || {
        let client = client_for(&server);
        async move { Ok(client) }
    })
    .await
    .expect_err("nothing was created");

    assert_ne!(code_of(&error), "", "{error:?}");
}

/// Connecting is the one thing `create` does first, once, and the request that
/// follows is a single call: a retry-looking double send would create twice.
#[tokio::test]
async fn one_command_is_one_request() {
    let server = MockServer::start_async().await;
    let mock = serve(&server, json!({ "data": { "createSkuGroup": created() } })).await;
    let connected = Cell::new(0);

    create(&args(None), false, || {
        connected.set(connected.get() + 1);
        let client = client_for(&server);
        async move { Ok(client) }
    })
    .await
    .expect("created");

    assert_eq!(connected.get(), 1);
    assert_eq!(mock.calls_async().await, 1);
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

/// A preview signs in and connects, as the real run must, but creates nothing: no
/// request reaches the API. What it would send is what it prints, only the
/// details given.
#[tokio::test]
async fn a_preview_creates_nothing_and_says_what_it_would_create() {
    let server = MockServer::start_async().await;
    let mock = serve(&server, json!({ "data": { "createSkuGroup": created() } })).await;
    let connected = Cell::new(0);
    let mut given = args(None);
    given.description = Some("Soft".to_owned());

    let result = create(&given, true, || {
        connected.set(connected.get() + 1);
        let client = client_for(&server);
        async move { Ok(client) }
    })
    .await
    .expect("a preview");

    assert!(result.metadata.dry_run, "marked as a preview");
    assert_eq!(
        connected.get(),
        1,
        "the real run needs it, so a preview does"
    );
    assert_eq!(mock.calls_async().await, 0, "nothing was created");
    assert_eq!(result.data["action"], "dry-run: would create product");
    assert_eq!(
        result.data["changes"],
        json!({ "label": "Classic Tee", "type": "PHYSICAL", "description": "Soft" })
    );
    assert!(
        result.metadata.next_actions.is_empty(),
        "nothing to read back"
    );
}

/// A preview that cannot connect fails as the real run would.
#[tokio::test]
async fn a_preview_fails_where_connecting_fails() {
    let error = create(&args(None), true, || async {
        Err(GddyError::config("no store selected").into_cli_error())
    })
    .await
    .expect_err("not connected");

    assert_eq!(code_of(&error), codes::CONFIG_ERROR);
}
