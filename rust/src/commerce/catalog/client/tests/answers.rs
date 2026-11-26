//! Tests for how the catalog client validates what the API answers with: an
//! answer that is not an answer to the query, a page that does not add up, and
//! the wording of the failures. Split from `tests.rs` for the repository's
//! per-file size limit; the helpers are that file's.

use super::*;

fn product_node() -> Value {
    json!({
        "id": "p-1", "name": "tee", "label": "Tee", "status": "ACTIVE", "type": "PHYSICAL",
        "description": null, "skuCount": 1,
        "createdAt": "2026-01-01T00:00:00Z", "updatedAt": "2026-01-02T00:00:00Z",
        "priceRange": null, "compareAtPriceRange": null
    })
}

fn page_with(edges: Value, page_info: Value, total: Value) -> Value {
    json!({ "data": { "skuGroups": {
        "totalCount": total, "pageInfo": page_info, "edges": edges
    } } })
}

async fn list_with_body(body: Value) -> Result<Option<product_list::ProductListSkuGroups>> {
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(POST);
            then.status(200).json_body(body);
        })
        .await;
    client_for(&server, STORE_ID)
        .list_products(10, None, None)
        .await
}

/// A successful answer has to be one: an empty `data` object, or one with none of
/// the fields that were asked for, is a broken response and not an empty
/// catalog. Only a `skuGroups` that is explicitly `null` means no products.
#[tokio::test]
async fn an_answer_without_the_requested_field_is_not_an_empty_catalog() {
    for (label, body) in [
        ("empty data", json!({ "data": {} })),
        (
            "data without the field",
            json!({ "data": { "somethingElse": 1 } }),
        ),
        ("data that is not an object", json!({ "data": 7 })),
        ("data that is a list", json!({ "data": [] })),
    ] {
        let error = list_with_body(body)
            .await
            .expect_err("an answer with nothing in it is a failure");

        assert_eq!(code_of(&error), codes::UNEXPECTED_ERROR, "{label}");
        assert!(text_of(&error).contains("cannot be used"), "{label}");
    }
}

#[tokio::test]
async fn an_explicit_null_connection_is_still_no_products_not_a_failure() {
    let page = list_with_body(json!({ "data": { "skuGroups": null } }))
        .await
        .expect("an explicit null is an answer");

    assert!(page.is_none());
}

/// A page that says there is more, with no cursor to get it, would send a caller
/// that pages on `hasNextPage` round again from the start, or stop early.
#[tokio::test]
async fn a_page_with_a_next_page_but_no_cursor_is_rejected() {
    for cursor in [Value::Null, json!(""), json!("   ")] {
        let body = page_with(
            json!([{ "node": product_node() }]),
            json!({ "hasNextPage": true, "endCursor": cursor }),
            json!(5),
        );

        let error = list_with_body(body)
            .await
            .expect_err("a next page nobody can reach is a failure");

        assert_eq!(code_of(&error), codes::UNEXPECTED_ERROR, "{cursor}");
    }
}

#[tokio::test]
async fn a_last_page_needs_no_cursor() {
    let body = page_with(
        json!([{ "node": product_node() }]),
        json!({ "hasNextPage": false, "endCursor": null }),
        json!(1),
    );

    let page = list_with_body(body)
        .await
        .expect("a last page without a cursor is fine")
        .expect("a connection");

    assert!(!page.page_info.has_next_page);
}

/// A null edge, or an edge with a null node, would drop a product from the list
/// without any sign that it had gone.
#[tokio::test]
async fn an_edge_that_does_not_describe_a_product_is_rejected() {
    for (label, edges) in [
        ("a null edge", json!([null])),
        ("a null node", json!([{ "node": null }])),
        (
            "one good and one null",
            json!([{ "node": product_node() }, null]),
        ),
    ] {
        let body = page_with(
            edges,
            json!({ "hasNextPage": false, "endCursor": null }),
            json!(2),
        );

        let error = list_with_body(body)
            .await
            .expect_err("a product that is not there is a failure");

        assert_eq!(code_of(&error), codes::UNEXPECTED_ERROR, "{label}");
    }
}

/// No edges, or a null list of them, is an empty page, which is a real answer.
#[tokio::test]
async fn a_page_with_no_edges_is_an_empty_page() {
    for edges in [json!([]), Value::Null] {
        let body = page_with(
            edges,
            json!({ "hasNextPage": false, "endCursor": null }),
            json!(0),
        );

        let page = list_with_body(body)
            .await
            .expect("an empty page is an answer")
            .expect("a connection");

        assert_eq!(page.edges.unwrap_or_default().len(), 0);
    }
}

/// The reasons are written in gddy: nothing from the answer is quoted, even when
/// the answer is the wrong shape and holds a value a merchant would not want in a
/// bug report.
#[tokio::test]
async fn a_malformed_answer_is_described_without_quoting_it() {
    let secret = "SECRET-GOV-ID-123-45-6789";
    for body in [
        json!({ "data": { "skuGroups": secret } }),
        json!({ "data": { "skuGroups": { "totalCount": secret, "pageInfo": {}, "edges": [] } } }),
        page_with(
            json!([{ "node": { "id": secret, "label": 1 } }]),
            json!({ "hasNextPage": secret, "endCursor": null }),
            json!(1),
        ),
    ] {
        let error = list_with_body(body.clone())
            .await
            .expect_err("a malformed answer is a failure");

        let text = format!("{:?}", build_error_envelope(&error, "commerce"));
        assert!(!text.contains(secret), "{body}: {text}");
        assert_eq!(code_of(&error), codes::UNEXPECTED_ERROR, "{body}");
        assert!(!fix_of(&error).contains("corrected input"), "{body}");
    }
}

/// Every root field a query selects must be in the answer, not just one of them.
/// An entry that is `null` counts: that is how a server says "nothing".
#[test]
fn an_answer_has_to_contain_every_field_that_was_asked_for() {
    for (data, fields, expected) in [
        (json!({ "a": 1, "b": 2 }), vec!["a", "b"], true),
        (json!({ "a": null, "b": null }), vec!["a", "b"], true),
        (json!({ "a": 1 }), vec!["a", "b"], false),
        (json!({ "b": 1 }), vec!["a", "b"], false),
        (json!({}), vec!["a"], false),
        (json!({ "a": 1, "extra": 2 }), vec!["a"], true),
        (json!(null), vec!["a"], false),
        (json!([1]), vec!["a"], false),
        (json!({ "a": 1 }), vec![], true),
    ] {
        assert_eq!(answers(&data, &fields), expected, "{data} {fields:?}");
    }
}
