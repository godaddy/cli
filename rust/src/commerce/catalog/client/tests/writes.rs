//! Tests for the client's mutations: what is sent, a product that is not there,
//! and what a refusal says. Split from `tests.rs` for the repository's per-file
//! size limit; the helpers are that file's.

use super::*;
use crate::commerce::catalog::operations::{product_create, product_update};

const PRODUCT_ID: &str = "0c3b1a52-77de-4c0e-8f6b-1d2a9e4f5b60";

fn stored() -> Value {
    json!({
        "id": PRODUCT_ID, "name": "tee", "label": "Tee", "status": "ACTIVE", "type": "PHYSICAL",
        "description": null, "htmlDescription": null, "skuCount": 1,
        "createdAt": "2026-01-01T00:00:00Z", "updatedAt": "2026-01-02T00:00:00Z",
        "activatedAt": null, "archivedAt": null,
        "priceRange": null, "compareAtPriceRange": null
    })
}

fn not_found() -> Value {
    json!({
        "errors": [{ "message": "SKU Group not found", "extensions": { "code": "NOT_FOUND" } }],
        "data": null
    })
}

async fn serve(server: &MockServer, body: Value) {
    server
        .mock_async(move |when, then| {
            when.method(POST);
            then.status(200).json_body(body.clone());
        })
        .await;
}

fn changes() -> product_update::MutationUpdateSkuGroupInput {
    product_update::MutationUpdateSkuGroupInput {
        label: Some("Tee".to_owned()),
        ..product_update::MutationUpdateSkuGroupInput::default()
    }
}

/// A mutation that names a product the store does not have is answered with a
/// `NOT_FOUND` error, where a read is answered with `null`. Both mean the product
/// is not there, so both come back as `None` and the caller says so once.
#[tokio::test]
async fn a_mutation_on_a_missing_product_is_none_not_a_failure() {
    let server = MockServer::start_async().await;
    serve(&server, not_found()).await;
    let client = client_for(&server, STORE_ID);

    assert!(
        client
            .update_product(PRODUCT_ID, changes())
            .await
            .expect("not a failure")
            .is_none()
    );
    assert!(
        client
            .archive_product(PRODUCT_ID, false)
            .await
            .expect("not a failure")
            .is_none()
    );
}

/// `NOT_FOUND` means "the product is not there" only when it is the whole story:
/// beside another error, the request failed for a reason that is not that.
#[tokio::test]
async fn not_found_beside_another_error_is_still_a_failure() {
    let server = MockServer::start_async().await;
    serve(
        &server,
        json!({ "errors": [
            { "message": "x", "extensions": { "code": "NOT_FOUND" } },
            { "message": "y", "extensions": { "code": "VALIDATION_ERROR" } }
        ], "data": null }),
    )
    .await;

    let error = client_for(&server, STORE_ID)
        .update_product(PRODUCT_ID, changes())
        .await
        .expect_err("a failure");

    assert_eq!(code_of(&error), codes::VALIDATION_ERROR);
}

/// Creating has no product to be missing, so a `NOT_FOUND` there is a failure and
/// is never read as "nothing to do".
#[tokio::test]
async fn not_found_on_create_is_a_failure() {
    let server = MockServer::start_async().await;
    serve(&server, not_found()).await;

    let error = client_for(&server, STORE_ID)
        .create_product(product_create::MutationCreateSkuGroupInput {
            label: "Tee".to_owned(),
            type_: "PHYSICAL".to_owned(),
            ..product_create::MutationCreateSkuGroupInput::default()
        })
        .await
        .expect_err("a failure");

    assert!(text_of(&error).contains("NOT_FOUND"), "{}", text_of(&error));
}

/// A `null` answer to a mutation is no product, and for create that is a failure:
/// nothing was made that can be shown.
#[tokio::test]
async fn a_create_answered_with_null_is_a_failure() {
    let server = MockServer::start_async().await;
    serve(&server, json!({ "data": { "createSkuGroup": null } })).await;

    let error = client_for(&server, STORE_ID)
        .create_product(product_create::MutationCreateSkuGroupInput {
            label: "Tee".to_owned(),
            type_: "PHYSICAL".to_owned(),
            ..product_create::MutationCreateSkuGroupInput::default()
        })
        .await
        .expect_err("a failure");

    assert_eq!(code_of(&error), codes::UNEXPECTED_ERROR);
    assert!(
        text_of(&error).contains("did not contain the product that was created"),
        "{}",
        text_of(&error)
    );
}

/// An answer to create that is not an answer to it is called that, and is not
/// mistaken for a create that made nothing.
#[tokio::test]
async fn a_create_answer_without_its_field_is_malformed() {
    let server = MockServer::start_async().await;
    serve(&server, json!({ "data": { "skuGroup": stored() } })).await;

    let error = client_for(&server, STORE_ID)
        .create_product(product_create::MutationCreateSkuGroupInput {
            label: "Tee".to_owned(),
            type_: "PHYSICAL".to_owned(),
            ..product_create::MutationCreateSkuGroupInput::default()
        })
        .await
        .expect_err("malformed");

    assert!(
        text_of(&error).contains("did not contain what was asked for"),
        "{}",
        text_of(&error)
    );
}

/// A mutation's answer must carry its own root field, like a query's, so an
/// answer to some other operation is not read as "the product is gone".
#[tokio::test]
async fn a_mutation_answer_without_its_field_is_malformed() {
    let server = MockServer::start_async().await;
    serve(&server, json!({ "data": { "skuGroup": stored() } })).await;
    let client = client_for(&server, STORE_ID);

    for error in [
        client
            .update_product(PRODUCT_ID, changes())
            .await
            .expect_err("malformed"),
        client
            .archive_product(PRODUCT_ID, true)
            .await
            .expect_err("malformed"),
    ] {
        assert_eq!(code_of(&error), codes::UNEXPECTED_ERROR);
    }
}

/// A mutation is sent once. The client has no retry, and a second send would
/// create a second product.
#[tokio::test]
async fn a_mutation_is_one_request() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(POST);
            then.status(200)
                .json_body(json!({ "data": { "createSkuGroup": stored() } }));
        })
        .await;

    client_for(&server, STORE_ID)
        .create_product(product_create::MutationCreateSkuGroupInput {
            label: "Tee".to_owned(),
            type_: "PHYSICAL".to_owned(),
            ..product_create::MutationCreateSkuGroupInput::default()
        })
        .await
        .expect("created");

    assert_eq!(mock.calls_async().await, 1);
}
