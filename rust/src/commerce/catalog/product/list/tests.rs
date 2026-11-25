use std::cell::Cell;

use super::*;
use crate::error::codes;
use cli_engine::{
    OutputSchema, PipelineOpts, apply_pipeline, build_error_envelope, preview_human_view,
};
use httpmock::{HttpMockRequest, prelude::*};
use reqwest::Client as ReqwestClient;
use serde_json::{from_slice, json};

const STORE_ID: &str = "6d1f0f4c-8a2e-4a52-9a5b-0f6a3c1d2e4b";

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

fn node(id: &str) -> Value {
    json!({
        "id": id, "name": format!("product-{id}"), "label": format!("Product {id}"),
        "status": "ACTIVE", "type": "PHYSICAL", "description": null, "skuCount": 2,
        "createdAt": "2026-01-01T00:00:00Z", "updatedAt": "2026-01-02T00:00:00Z",
        "priceRange": { "min": 9.5, "max": 20.0 }, "compareAtPriceRange": null
    })
}

fn page(ids: &[&str], next: Option<&str>, total: usize) -> Value {
    json!({ "data": { "skuGroups": {
        "totalCount": total,
        "pageInfo": { "hasNextPage": next.is_some(), "endCursor": next },
        "edges": ids.iter().map(|id| json!({ "node": node(id) })).collect::<Vec<_>>()
    } } })
}

fn variable(request: &HttpMockRequest, pointer: &str) -> Option<Value> {
    from_slice::<Value>(request.body_ref())
        .ok()?
        .pointer(&format!("/variables/{pointer}"))
        .cloned()
}

/// A server that answers the request carrying cursor `c{n}` with `chain[n]`, and
/// one with no cursor with `chain[0]`.
async fn serve_chain(server: &MockServer, chain: Vec<Value>) {
    for (index, body) in chain.into_iter().enumerate() {
        let cursor = (index > 0).then(|| json!(format!("c{index}")));
        server
            .mock_async(move |when, then| {
                when.method(POST)
                    .is_true(move |request| variable(request, "after") == cursor);
                then.status(200).json_body(body.clone());
            })
            .await;
    }
}

async fn first_page(body: Value) -> (Vec<ProductSummary>, CursorContinuation) {
    let server = MockServer::start_async().await;
    serve_chain(&server, vec![body]).await;
    list_page(&client_for(&server), &[], 25, None)
        .await
        .expect("a page")
}

fn statuses(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
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

// --- paging ---------------------------------------------------------------

/// One page is one request: the rest of the catalog is not read, and the page
/// says where it leaves off.
#[tokio::test]
async fn a_page_is_one_request_and_says_where_it_leaves_off() {
    let server = MockServer::start_async().await;
    let first = server
        .mock_async(|when, then| {
            when.method(POST)
                .is_true(|request| variable(request, "after").is_none());
            then.status(200).json_body(page(&["a", "b"], Some("c1"), 5));
        })
        .await;
    let second = server
        .mock_async(|when, then| {
            when.method(POST)
                .is_true(|request| variable(request, "after") == Some(json!("c1")));
            then.status(200).json_body(page(&["c"], None, 5));
        })
        .await;

    let (products, continuation) = list_page(&client_for(&server), &[], 2, None)
        .await
        .expect("the first page");

    assert_eq!(products.len(), 2);
    assert_eq!(continuation.continue_from.as_deref(), Some("|c1"));
    assert_eq!(continuation.total, Some(5));
    assert_eq!(first.calls_async().await, 1);
    assert_eq!(second.calls_async().await, 0, "the next page is not read");
}

/// The page size asked for is the page size sent.
#[tokio::test]
async fn the_page_size_is_sent_as_the_limit() {
    for limit in [1, 25, 100] {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(move |when, then| {
                when.method(POST)
                    .is_true(move |request| variable(request, "first") == Some(json!(limit)));
                then.status(200).json_body(page(&[], None, 0));
            })
            .await;

        list_page(&client_for(&server), &[], limit, None)
            .await
            .expect("a page");

        mock.assert_async().await;
    }
}

/// Following the token from one page to the next reads the whole catalog, each
/// product once, and the last page ends the list.
#[tokio::test]
async fn following_the_tokens_walks_the_catalog_once() {
    let server = MockServer::start_async().await;
    serve_chain(
        &server,
        vec![
            page(&["a", "b"], Some("c1"), 5),
            page(&["c", "d"], Some("c2"), 5),
            page(&["e"], None, 5),
        ],
    )
    .await;
    let client = client_for(&server);
    let filter = statuses(&["ACTIVE"]);

    let mut seen = Vec::new();
    let mut token: Option<String> = None;
    for _ in 0..10 {
        let after = resume_cursor(token.as_deref(), &filter).expect("our own token resumes");
        let (products, continuation) = list_page(&client, &filter, 2, after.as_deref())
            .await
            .expect("a page");
        seen.extend(products.into_iter().map(|product| product.id));
        token = continuation.continue_from;
        if token.is_none() {
            break;
        }
    }

    assert_eq!(seen, ["a", "b", "c", "d", "e"]);
    assert!(token.is_none(), "the last page has no token");
}

#[tokio::test]
async fn the_last_page_has_no_token_even_when_it_is_full() {
    let (products, continuation) = first_page(page(&["a", "b"], None, 2)).await;

    assert_eq!(products.len(), 2);
    assert_eq!(continuation.continue_from, None);
}

/// A last page still has an end cursor, since it is the cursor of its last
/// product. What says there is nothing after it is `hasNextPage`, so a token
/// built from the cursor alone would send the user to an empty page.
#[tokio::test]
async fn a_last_page_that_has_an_end_cursor_still_has_no_token() {
    let body = json!({ "data": { "skuGroups": {
        "totalCount": 2,
        "pageInfo": { "hasNextPage": false, "endCursor": "last-product-cursor" },
        "edges": [{ "node": node("a") }, { "node": node("b") }]
    } } });

    let (products, continuation) = first_page(body).await;

    assert_eq!(products.len(), 2);
    assert_eq!(continuation.continue_from, None);
}

#[tokio::test]
async fn an_empty_store_is_an_empty_page_not_an_error() {
    let (products, continuation) = first_page(page(&[], None, 0)).await;

    assert!(products.is_empty());
    assert_eq!(continuation.continue_from, None);
}

#[tokio::test]
async fn a_store_with_no_products_connection_is_an_empty_page_too() {
    let (products, continuation) = first_page(json!({ "data": { "skuGroups": null } })).await;

    assert!(products.is_empty());
    assert_eq!(continuation.continue_from, None);
}

/// The status filter is sent as the API takes it, and left out when none is
/// given, so every status is listed.
#[tokio::test]
async fn the_status_filter_is_sent_only_when_given() {
    for (given, expected) in [
        (vec![], None),
        (
            statuses(&["ACTIVE", "DRAFT"]),
            Some(json!({ "in": ["ACTIVE", "DRAFT"] })),
        ),
    ] {
        let server = MockServer::start_async().await;
        let wanted = expected.clone();
        let mock = server
            .mock_async(move |when, then| {
                when.method(POST)
                    .is_true(move |request| variable(request, "status") == wanted);
                then.status(200).json_body(page(&[], None, 0));
            })
            .await;

        list_page(&client_for(&server), &given, 25, None)
            .await
            .expect("a page");

        mock.assert_async().await;
    }
}

// --- the --continue token -------------------------------------------------

/// Leaving `--continue` off is the only way to start from the first page.
#[test]
fn no_token_starts_from_the_first_page() {
    assert_eq!(resume_cursor(None, &[]).expect("starts over"), None);
}

/// A `--continue` that was given but is blank is what `--continue "$TOKEN"` becomes
/// when the variable is empty. It is an error, not a quiet return to page one.
#[test]
fn a_blank_token_is_refused_and_not_taken_for_the_first_page() {
    for token in ["", " ", "   ", "\t"] {
        for filter in [statuses(&[]), statuses(&["ACTIVE"])] {
            let error = resume_cursor(Some(token), &filter)
                .expect_err(&format!("{token:?} is not a token"));

            let text = text_of(&error);
            assert_eq!(code_of(&error), codes::VALIDATION_ERROR, "{token:?}");
            assert!(text.contains("leave --continue off"), "{token:?}: {text}");
        }
    }
}

/// A token resumes only the list it was issued for, however the same filter is
/// spelled: order and repeats do not change which list it is.
#[test]
fn a_token_resumes_the_list_it_was_issued_for() {
    let issued = issue_token(&statuses(&["DRAFT", "ACTIVE"]), "c1");

    for same in [
        statuses(&["ACTIVE", "DRAFT"]),
        statuses(&["DRAFT", "ACTIVE"]),
        statuses(&["ACTIVE", "DRAFT", "ACTIVE"]),
    ] {
        assert_eq!(
            resume_cursor(Some(&issued), &same).expect("the same list"),
            Some("c1".to_owned()),
            "{same:?}"
        );
    }
}

/// A cursor is a position in one list. Used with another filter it would be a
/// position in a list it was never issued for, so it is refused, and the error
/// says what the token was for.
#[test]
fn a_token_for_another_filter_is_refused_and_says_so() {
    let issued = issue_token(&statuses(&["ACTIVE"]), "c1");

    for other in [
        statuses(&[]),
        statuses(&["DRAFT"]),
        statuses(&["ACTIVE", "DRAFT"]),
    ] {
        let error = resume_cursor(Some(&issued), &other).expect_err("another list");

        let text = text_of(&error);
        assert_eq!(code_of(&error), codes::VALIDATION_ERROR, "{other:?}");
        assert!(text.contains("--status active"), "{text}");
        assert!(text.contains("same --status"), "{text}");
    }
    let unfiltered = issue_token(&[], "c1");
    let error = resume_cursor(Some(&unfiltered), &statuses(&["ACTIVE"]))
        .expect_err("a token for the whole list is not for a filtered one");
    assert!(
        text_of(&error).contains("no --status"),
        "{}",
        text_of(&error)
    );
}

/// Anything that is not a token this command printed is an error, not a quiet
/// return to the first page: a copy-paste slip must be seen as one. Each case
/// is run with the filter that would otherwise let it through, and the message
/// is checked, since "this token is for another list" is a different and wrong
/// diagnosis of a token that is not one at all.
#[test]
fn a_token_this_command_did_not_issue_is_refused() {
    let none = statuses(&[]);
    let active = statuses(&["ACTIVE"]);
    for (token, filter) in [
        // no separator: with no filter this is what a bare API cursor looks like
        ("garbage", &none),
        ("c1", &none),
        ("garbage", &active),
        // no cursor after the separator
        ("|", &none),
        ("ACTIVE|", &active),
        // a filter part that is not made of statuses this command issues
        ("BOGUS|c1", &none),
        ("BOGUS|c1", &active),
        ("ACTIVE,BOGUS|c1", &active),
        ("active|c1", &active),
        ("ACTIVE ,DRAFT|c1", &active),
        ("ACTIVE,,DRAFT|c1", &none),
    ] {
        let error = resume_cursor(Some(token), filter)
            .expect_err(&format!("{token:?} with {filter:?} is not a token"));

        let text = text_of(&error);
        assert_eq!(code_of(&error), codes::VALIDATION_ERROR, "{token:?}");
        assert!(
            text.contains("is not one `product list` printed"),
            "{token:?} with {filter:?} should be called not a token: {text}"
        );
        assert!(text.contains("leave --continue off"), "{token:?}: {text}");
        assert!(!text.contains("was issued for"), "{token:?}: {text}");
    }
}

/// The API's cursor is opaque and may contain anything, including the
/// separator, so only the first separator splits the token.
#[test]
fn a_cursor_containing_the_separator_survives_the_round_trip() {
    for cursor in ["a|b", "||", "x|", "=="] {
        let token = issue_token(&[], cursor);

        assert_eq!(
            resume_cursor(Some(&token), &[]).expect("round trips"),
            Some(cursor.to_owned()),
            "{cursor:?}"
        );
    }
}

#[test]
fn a_status_is_accepted_in_any_case_and_sent_in_upper_case() {
    for (given, sent) in [
        ("active", "ACTIVE"),
        ("Draft", "DRAFT"),
        (" ARCHIVED ", "ARCHIVED"),
    ] {
        assert_eq!(parse_status(given), Ok(sent.to_owned()));
    }
}

#[test]
fn an_unknown_status_is_rejected_and_says_what_is_accepted() {
    for given in ["", "bogus", "act", "ACTIVE,DRAFT", "deleted"] {
        let error = parse_status(given).expect_err("not a status");
        for status in ["draft", "active", "archived"] {
            assert!(error.contains(status), "{given:?}: {error}");
        }
    }
}

// --- the command's shape --------------------------------------------------

/// The command uses the engine's cursor pagination and not its offset pagination,
/// with the page size the API allows.
#[test]
fn the_command_pages_with_a_cursor_not_an_offset() {
    let spec = command().spec;

    assert_eq!(spec.cursor, Some(CursorConfig::new(25, 100)));
    assert!(spec.pagination.is_none(), "offset paging is not registered");
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

/// What the engine needs to print the footer and the next-page command: the
/// token, and the total, travel on the result.
#[test]
fn the_result_carries_where_the_page_leaves_off() {
    let with_more = list_result(
        vec![json!({ "id": "a" })],
        CursorContinuation::more("|c1").with_total(9),
        None,
    );
    let last = list_result(vec![json!({ "id": "a" })], CursorContinuation::done(), None);

    let cursor = with_more.metadata.cursor.expect("a cursor");
    assert_eq!(cursor.continue_from.as_deref(), Some("|c1"));
    assert_eq!(cursor.total, Some(9));
    assert_eq!(last.metadata.cursor.expect("a cursor").continue_from, None);
}

/// "Show one product" is offered when there are products, and not for an empty
/// page, where it would send the user to a command with nothing to run it on.
#[test]
fn the_next_step_is_only_offered_when_there_is_a_product_to_show() {
    let with = list_result(vec![json!({ "id": "a" })], CursorContinuation::done(), None);
    let without = list_result(vec![], CursorContinuation::done(), None);

    assert_eq!(with.metadata.next_actions.len(), 1);
    assert!(without.metadata.next_actions.is_empty());
}

/// The suggested `get` names the store the list was for when `--store-id` chose
/// it, with the id as a structured parameter, so the next command reads the same
/// store and not the selected one. With no `--store-id` it names none, since the
/// selected store is what it will use.
#[test]
fn the_next_step_names_the_store_only_when_the_list_was_for_one_given() {
    let page = || vec![json!({ "id": "a" })];

    let given = list_result(page(), CursorContinuation::done(), Some(STORE_ID));
    let action = &given.metadata.next_actions[0];
    assert!(
        action
            .command
            .ends_with("commerce catalog product get <product-id> --store-id <store-id>"),
        "{}",
        action.command
    );
    let store = &action.params["store-id"];
    assert_eq!(store.value.as_deref(), Some(STORE_ID));
    assert!(store.required);

    let selected = list_result(page(), CursorContinuation::done(), None);
    let action = &selected.metadata.next_actions[0];
    assert!(!action.command.contains("--store-id"), "{}", action.command);
    assert!(!action.params.contains_key("store-id"));
}

/// The product to show is the one thing the user must supply, so it is declared
/// as a required parameter with the placeholder's own name.
#[test]
fn the_next_step_declares_the_product_id_as_a_required_parameter() {
    for store in [None, Some(STORE_ID)] {
        let result = list_result(
            vec![json!({ "id": "a" })],
            CursorContinuation::done(),
            store,
        );

        let action = &result.metadata.next_actions[0];
        assert!(
            action.command.contains("<product-id>"),
            "{}",
            action.command
        );
        let param = &action.params["product-id"];
        assert!(param.required);
        assert_eq!(param.value, None, "the user picks the product");
    }
}

// --- the handler ----------------------------------------------------------

fn list_args(store_id: Option<&str>, status: &[&str]) -> ListArgs {
    ListArgs {
        store: StoreIdArg {
            store_id: store_id.map(str::to_owned),
        },
        status: statuses(status),
    }
}

/// A token that is not usable is refused before the command reaches the API, so a
/// bad `--continue` costs no request and does not need a sign-in.
#[tokio::test]
async fn a_bad_token_fails_before_the_api_is_reached() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(POST);
            then.status(200).json_body(page(&["a"], None, 1));
        })
        .await;
    let connected = Cell::new(false);

    for token in ["", "   ", "garbage", "ACTIVE|c1"] {
        let error = list(&list_args(None, &[]), Some(token), 25, || {
            connected.set(true);
            let client = client_for(&server);
            async move { Ok(client) }
        })
        .await
        .expect_err("a bad token is refused");

        assert_eq!(code_of(&error), codes::VALIDATION_ERROR, "{token:?}");
    }

    assert!(!connected.get(), "the API was connected to");
    assert_eq!(mock.calls_async().await, 0);
}

/// The handler as a whole: a valid token is resumed from, and the result names the
/// store the list was for.
#[tokio::test]
async fn a_valid_token_resumes_the_list_and_the_result_names_its_store() {
    let server = MockServer::start_async().await;
    serve_chain(
        &server,
        vec![page(&["a"], Some("c1"), 2), page(&["b"], None, 2)],
    )
    .await;

    let result = list(
        &list_args(Some(STORE_ID), &["ACTIVE"]),
        Some("ACTIVE|c1"),
        1,
        || {
            let client = client_for(&server);
            async move { Ok(client) }
        },
    )
    .await
    .expect("the second page");

    assert_eq!(result.data[0]["id"], "b");
    assert_eq!(
        result.metadata.cursor.expect("a cursor").continue_from,
        None
    );
    assert_eq!(
        result.metadata.next_actions[0].params["store-id"]
            .value
            .as_deref(),
        Some(STORE_ID)
    );
}

// --- what is shown --------------------------------------------------------

/// What `list` prints is a merchant's view of a product: renamed fields, no API
/// jargon, and amounts only when the product has a price.
#[tokio::test]
async fn products_are_shown_in_merchant_terms() {
    let (products, _) = first_page(page(&["a"], None, 1)).await;
    let shown = to_value(&products[0]).expect("serializes");

    assert_eq!(shown["variantCount"], 2);
    assert_eq!(shown["priceMin"], 9.5);
    assert_eq!(shown["priceMax"], 20.0);
    assert_eq!(shown["type"], "PHYSICAL");
    assert!(shown.get("priceRange").is_none(), "{shown}");
    assert!(shown.get("skuCount").is_none(), "{shown}");
    assert!(shown.get("sku_count").is_none(), "{shown}");
}

/// A product the API gives no price range for (`null`, which is what an unpriced
/// product gets) has no price, not a price of zero.
#[tokio::test]
async fn a_product_with_a_null_price_range_has_no_price_not_zero() {
    let mut bare = node("a");
    bare["priceRange"] = Value::Null;
    bare["compareAtPriceRange"] = Value::Null;
    let body = json!({ "data": { "skuGroups": {
        "totalCount": 1,
        "pageInfo": { "hasNextPage": false, "endCursor": null },
        "edges": [{ "node": bare }]
    } } });

    let (products, _) = first_page(body).await;

    let shown = to_value(&products[0]).expect("serializes");
    for field in [
        "priceMin",
        "priceMax",
        "compareAtPriceMin",
        "compareAtPriceMax",
    ] {
        assert!(shown[field].is_null(), "{field} should be absent: {shown}");
    }
}

/// Through the engine's own default projection: every field the default fields
/// name has to be one the command emits, or it is silently missing from output.
#[tokio::test]
async fn every_default_field_is_one_the_command_emits() {
    let (products, _) = first_page(page(&["a"], None, 1)).await;
    let mut data = Value::Array(vec![to_value(&products[0]).expect("serializes")]);
    let fields = command().spec.default_fields.expect("default fields");

    apply_pipeline(
        &mut data,
        &PipelineOpts {
            fields: fields.clone(),
            fields_are_default: true,
            ..PipelineOpts::default()
        },
    )
    .expect("the default projection applies");

    for field in fields.split(',') {
        assert!(
            data[0].get(field).is_some_and(|v| !v.is_null()),
            "{field} is a default field but is missing from the output: {data}"
        );
    }
}

/// What `--schema` and `--help` advertise must be what the command emits.
#[tokio::test]
async fn the_advertised_fields_match_what_the_command_emits() {
    let (products, _) = first_page(page(&["a"], None, 1)).await;
    let shown = to_value(&products[0]).expect("serializes");
    let mut emitted: Vec<&str> = shown
        .as_object()
        .expect("object")
        .keys()
        .map(String::as_str)
        .collect();
    let mut advertised: Vec<&str> = ProductListItem::fields().iter().map(|f| f.name).collect();
    emitted.sort_unstable();
    advertised.sort_unstable();

    assert_eq!(advertised, emitted);
}

/// A person sees the columns in merchant terms, with the amounts as plain numbers
/// and nothing from the API's own vocabulary.
#[tokio::test]
async fn the_list_renders_for_a_person_in_merchant_terms() {
    let (products, _) = first_page(page(&["a"], None, 1)).await;
    let rows = Value::Array(vec![to_value(&products[0]).expect("serializes")]);

    let rendered = preview_human_view(rows, &view_columns());

    for heading in ["ID", "NAME", "STATUS", "VARIANTS", "PRICE FROM", "PRICE TO"] {
        assert!(rendered.contains(heading), "{heading} missing: {rendered}");
    }
    assert!(rendered.contains("9.5"), "{rendered}");
    assert!(rendered.contains("20"), "{rendered}");
    for jargon in ["SKU", "sku", "{\"", "priceRange"] {
        assert!(!rendered.contains(jargon), "{jargon} leaked: {rendered}");
    }
}

#[test]
fn an_empty_list_renders_as_no_results() {
    let rendered = preview_human_view(json!([]), &view_columns());
    assert!(rendered.contains("no results"), "{rendered}");
}

/// Every column the view names is a field the command advertises, or it would be
/// a heading over nothing.
#[test]
fn every_column_is_a_field_the_command_advertises() {
    let advertised: Vec<&str> = ProductListItem::fields().iter().map(|f| f.name).collect();
    for column in view_columns() {
        assert!(
            advertised.contains(&column.field.as_str()),
            "{} is a column but not a field",
            column.field
        );
    }
}
