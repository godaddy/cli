use super::*;
use crate::error::codes;
use cli_engine::{
    OutputSchema, PipelineOpts, apply_pipeline, build_error_envelope, preview_human_view,
};
use httpmock::prelude::*;
use reqwest::Client as ReqwestClient;
use serde_json::json;

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

fn product() -> Value {
    json!({
        "id": PRODUCT_ID, "name": "classic-tee", "label": "Classic T-Shirt",
        "status": "ACTIVE", "type": "PHYSICAL", "description": "Soft cotton",
        "htmlDescription": "<p>Soft cotton</p>", "skuCount": 3,
        "createdAt": "2026-01-01T00:00:00Z", "updatedAt": "2026-01-02T00:00:00Z",
        "activatedAt": "2026-01-03T00:00:00Z", "archivedAt": null,
        "priceRange": { "min": 19.0, "max": 29.5 },
        "compareAtPriceRange": { "min": 25.0, "max": 35.0 }
    })
}

async fn serve(server: &MockServer, skugroup: Value) {
    server
        .mock_async(move |when, then| {
            when.method(POST);
            then.status(200)
                .json_body(json!({ "data": { "skuGroup": skugroup.clone() } }));
        })
        .await;
}

#[tokio::test]
async fn a_product_is_shown_in_merchant_terms() {
    let server = MockServer::start_async().await;
    serve(&server, product()).await;

    let result = show(&client_for(&server), PRODUCT_ID, "ote")
        .await
        .expect("the product is shown");

    let data = &result.data;
    assert_eq!(data["id"], PRODUCT_ID);
    assert_eq!(data["label"], "Classic T-Shirt");
    assert_eq!(data["type"], "PHYSICAL");
    assert_eq!(data["variantCount"], 3);
    assert_eq!(data["priceMin"], 19.0);
    assert_eq!(data["priceMax"], 29.5);
    assert_eq!(data["compareAtPriceMin"], 25.0);
    assert_eq!(data["compareAtPriceMax"], 35.0);
    assert!(data.get("priceRange").is_none(), "{data}");
    assert_eq!(data["htmlDescription"], "<p>Soft cotton</p>");
    assert_eq!(data["activatedAt"], "2026-01-03T00:00:00Z");
    assert!(data.get("skuCount").is_none(), "{data}");
}

/// A product the store does not have is a not-found error that names it and says
/// how to find a real one, not an empty result and not the API's own wording.
#[tokio::test]
async fn a_missing_product_is_a_clear_not_found() {
    let server = MockServer::start_async().await;
    serve(&server, Value::Null).await;

    let error = show(&client_for(&server), PRODUCT_ID, "ote")
        .await
        .expect_err("a missing product is an error");

    let envelope = build_error_envelope(&error, "commerce");
    let message = envelope.error.as_ref().map(|e| e.message.clone());
    let message = message.unwrap_or_default();
    assert_eq!(
        envelope.error.as_ref().map(|e| e.code.as_str()),
        Some(codes::NOT_FOUND)
    );
    assert!(message.contains(PRODUCT_ID), "{message}");
    assert!(message.contains("ote"), "{message}");
    assert!(
        envelope
            .fix
            .as_deref()
            .is_some_and(|fix| fix.contains("product list")),
        "{envelope:?}"
    );
}

#[test]
fn a_product_id_is_checked_before_anything_is_sent() {
    for given in [
        "",
        "nope",
        "not-a-uuid",
        "6d1f0f4c",
        "6d1f0f4c-8a2e-4a52-9a5b",
    ] {
        let error = parse_product_id(given).expect_err("not a product id");
        let envelope = build_error_envelope(&error, "commerce");
        assert_eq!(
            envelope.error.map(|e| e.code),
            Some(codes::VALIDATION_ERROR.to_owned()),
            "{given:?}"
        );
        assert!(
            envelope
                .fix
                .as_deref()
                .is_some_and(|fix| fix.contains("product list")),
            "{given:?}"
        );
    }
}

/// The same product in another case or without hyphens is the same product, so
/// it is sent in the form the API takes.
#[test]
fn a_product_id_is_sent_hyphenated_and_in_lower_case() {
    for given in [
        PRODUCT_ID.to_uppercase(),
        PRODUCT_ID.replace('-', ""),
        format!("  {PRODUCT_ID}  "),
    ] {
        assert_eq!(
            parse_product_id(&given).expect("valid"),
            PRODUCT_ID,
            "{given:?}"
        );
    }
}

#[tokio::test]
async fn a_malformed_id_never_reaches_the_api() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(POST);
            then.status(200)
                .json_body(json!({ "data": { "skuGroup": null } }));
        })
        .await;

    let _ = parse_product_id("not-a-uuid");

    assert_eq!(mock.calls_async().await, 0);
}

/// Every field the command emits must survive the default projection, or it is
/// silently missing from default output.
#[tokio::test]
async fn the_default_output_shows_every_field_the_command_emits() {
    let server = MockServer::start_async().await;
    serve(&server, product()).await;
    let result = show(&client_for(&server), PRODUCT_ID, "ote")
        .await
        .expect("shown");
    let emitted: Vec<String> = result
        .data
        .as_object()
        .expect("an object")
        .keys()
        .cloned()
        .collect();
    let mut data = result.data.clone();
    if let Some(fields) = command().spec.default_fields {
        apply_pipeline(
            &mut data,
            &PipelineOpts {
                fields,
                fields_are_default: true,
                ..PipelineOpts::default()
            },
        )
        .expect("the default projection applies");
    }

    for field in &emitted {
        assert!(
            data.get(field).is_some(),
            "{field} is emitted but not shown: {data}"
        );
    }
}

#[tokio::test]
async fn the_advertised_fields_match_what_the_command_emits() {
    let server = MockServer::start_async().await;
    serve(&server, product()).await;
    let result = show(&client_for(&server), PRODUCT_ID, "ote")
        .await
        .expect("shown");
    let mut emitted: Vec<&str> = result
        .data
        .as_object()
        .expect("an object")
        .keys()
        .map(String::as_str)
        .collect();
    let mut advertised: Vec<&str> = ProductResult::fields().iter().map(|f| f.name).collect();
    emitted.sort_unstable();
    advertised.sort_unstable();

    assert_eq!(advertised, emitted);
}

/// A person sees a product's fields labelled in merchant terms, with the amounts
/// as plain numbers.
#[tokio::test]
async fn a_product_renders_for_a_person_in_merchant_terms() {
    let server = MockServer::start_async().await;
    serve(&server, product()).await;
    let result = show(&client_for(&server), PRODUCT_ID, "ote")
        .await
        .expect("shown");

    let rendered = preview_human_view(result.data, &view_columns());

    for label in [
        "Name: Classic T-Shirt",
        "Price from: 19",
        "Price to: 29.5",
        "Variants: 3",
    ] {
        assert!(rendered.contains(label), "{label} missing: {rendered}");
    }
    for jargon in ["SKU", "sku", "{\"", "priceRange"] {
        assert!(!rendered.contains(jargon), "{jargon} leaked: {rendered}");
    }
}

/// Every column the view names is a field the command advertises.
#[test]
fn every_column_is_a_field_the_command_advertises() {
    let advertised: Vec<&str> = ProductResult::fields().iter().map(|f| f.name).collect();
    for column in view_columns() {
        assert!(
            advertised.contains(&column.field.as_str()),
            "{} is a column but not a field",
            column.field
        );
    }
}

#[test]
fn the_command_declares_the_product_read_scope_and_is_read_only() {
    let spec = command().spec;
    assert_eq!(spec.tier, Some(Tier::Read));
    assert_eq!(
        spec.metadata().scopes,
        vec![COMMERCE_PRODUCT_READ.to_owned()]
    );
}
