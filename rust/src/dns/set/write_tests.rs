use super::*;
use serde_json::{Value, json};

// --- write_with_conflict_handling ---------------------------------------
//
// These exercise the DUPLICATE_RECORD branching against a mock server: the
// no-conflict path, the conflict-without-`--replace-conflicting-types`
// path (message only, no delete attempted), and the conflict-with-the-flag
// path (delete_conflicts outcome then the retry outcome, in that order).

use httpmock::prelude::*;

fn client_for(server: &MockServer) -> domains_client::Client {
    domains_client::client_with_builder(
        reqwest::Client::builder(),
        &server.base_url(),
        "Bearer tok",
        "req-1",
    )
    .expect("build client")
}

fn write_opts() -> RecordOptions {
    RecordOptions {
        ttl: None,
        priority: None,
        port: None,
        weight: None,
        protocol: None,
        service: None,
        flag: None,
        tag: None,
        usage: None,
        selector: None,
        matching_type: None,
        parameters: None,
    }
}

const DUPLICATE_RECORD_BODY: &str = r#"{"correlationId":"c-1","details":[{"issue":"DUPLICATE_RECORD","description":"Duplicate data provided for record name, www."}],"message":"Request failed validation","name":"VALIDATION_ERROR"}"#;

#[tokio::test]
async fn write_with_conflict_handling_creates_without_conflict() {
    let server = MockServer::start_async().await;
    let create = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/v3/domains/zones/example.com/dns-records");
            then.status(201).json_body(json!({
                "type": "A", "name": "www", "data": "1.2.3.4", "ttl": 3600,
                "recordId": "new-1"
            }));
        })
        .await;

    let opts = write_opts();
    let req = WriteRequest {
        domain: "example.com",
        name: "www",
        record_type: "A",
        value: "1.2.3.4",
        opts: &opts,
        replace_conflicting: false,
        replacing: None,
        debug: false,
    };
    let outcomes = write_with_conflict_handling(&client_for(&server), &req).await;

    create.assert_async().await;
    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].kind, "created");
    assert!(outcomes[0].error.is_none(), "{:?}", outcomes[0].error);
    // The id the API assigned reaches the result, so a real run can report it.
    assert_eq!(outcomes[0].record_id.as_deref(), Some("new-1"));
    assert_eq!(
        outcomes[0]
            .new
            .as_ref()
            .and_then(|r| r.record_id.as_deref()),
        Some("new-1")
    );
}

#[tokio::test]
async fn write_with_conflict_handling_sends_https_parameters_in_request_body() {
    let server = MockServer::start_async().await;
    let create = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/v3/domains/zones/example.com/dns-records")
                .json_body(json!({
                    "data": ".",
                    "name": "@",
                    "parameters": "alpn=h2,h3",
                    "priority": 1,
                    "ttl": 3600,
                    "type": "HTTPS"
                }));
            then.status(201)
                .json_body(json!({ "type": "HTTPS", "name": "@", "data": ".", "ttl": 3600 }));
        })
        .await;

    let mut opts = write_opts();
    opts.priority = Some(1);
    opts.parameters = Some("alpn=h2,h3".to_string());
    let req = WriteRequest {
        domain: "example.com",
        name: "@",
        record_type: "HTTPS",
        value: ".",
        opts: &opts,
        replace_conflicting: false,
        replacing: None,
        debug: false,
    };
    let outcomes = write_with_conflict_handling(&client_for(&server), &req).await;

    create.assert_async().await;
    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].kind, "created");
    assert!(outcomes[0].error.is_none(), "{:?}", outcomes[0].error);
}

#[tokio::test]
async fn write_with_conflict_handling_sends_tlsa_fields_in_request_body() {
    let server = MockServer::start_async().await;
    let cert = "d2abde240d7cd3ee6b4b28c54df034b97983a1d16e8a410e4561cb106618e971";
    let create = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/v3/domains/zones/example.com/dns-records")
                .json_body(json!({
                    "certificateData": cert,
                    "matchingType": 1,
                    "name": "www",
                    "port": 443,
                    "protocol": "_tcp",
                    "selector": 1,
                    "ttl": 3600,
                    "type": "TLSA",
                    "usage": 3
                }));
            then.status(201)
                .json_body(json!({ "type": "TLSA", "name": "www", "ttl": 3600 }));
        })
        .await;

    let mut opts = write_opts();
    opts.usage = Some(3);
    opts.selector = Some(1);
    opts.matching_type = Some(1);
    opts.protocol = Some("_tcp".to_string());
    opts.port = Some(443);
    let req = WriteRequest {
        domain: "example.com",
        name: "www",
        record_type: "TLSA",
        value: cert,
        opts: &opts,
        replace_conflicting: false,
        replacing: None,
        debug: false,
    };
    let outcomes = write_with_conflict_handling(&client_for(&server), &req).await;

    create.assert_async().await;
    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].kind, "created");
    assert!(outcomes[0].error.is_none(), "{:?}", outcomes[0].error);
}

#[tokio::test]
async fn write_with_conflict_handling_conflict_without_flag_reports_message_and_skips_delete() {
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/v3/domains/zones/example.com/dns-records");
            then.status(422).json_body(
                serde_json::from_str::<Value>(DUPLICATE_RECORD_BODY).expect("valid json"),
            );
        })
        .await;
    server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/v3/domains/zones/example.com/dns-records")
                .query_param("name", "www");
            then.status(200).json_body(json!({
                "items": [
                    { "type": "CNAME", "name": "www", "data": "parkpage.godaddy.com", "ttl": 3600, "recordId": "cn-1" }
                ],
                "totalItems": 1,
                "totalPages": 1,
                "links": []
            }));
        })
        .await;

    let opts = write_opts();
    let req = WriteRequest {
        domain: "example.com",
        name: "www",
        record_type: "A",
        value: "1.2.3.4",
        opts: &opts,
        replace_conflicting: false,
        replacing: None,
        debug: false,
    };
    let outcomes = write_with_conflict_handling(&client_for(&server), &req).await;

    // Only the message outcome — no delete attempted since the flag wasn't set.
    assert_eq!(outcomes.len(), 1);
    let err = outcomes[0].error.as_deref().expect("conflict reported");
    assert!(err.contains("already has a CNAME record"), "{err}");
    assert!(err.contains("--replace-conflicting-types"), "{err}");
}

#[tokio::test]
async fn write_with_conflict_handling_replace_flag_deletes_then_retries_in_order() {
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/v3/domains/zones/example.com/dns-records");
            then.status(422).json_body(
                serde_json::from_str::<Value>(DUPLICATE_RECORD_BODY).expect("valid json"),
            );
        })
        .await;
    server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/v3/domains/zones/example.com/dns-records")
                .query_param("name", "www");
            then.status(200).json_body(json!({
                "items": [
                    { "type": "CNAME", "name": "www", "data": "parkpage.godaddy.com", "ttl": 3600, "recordId": "cn-1" }
                ],
                "totalItems": 1,
                "totalPages": 1,
                "links": []
            }));
        })
        .await;
    let delete = server
        .mock_async(|when, then| {
            when.method(DELETE)
                .path("/v3/domains/zones/example.com/dns-records/cn-1");
            then.status(204);
        })
        .await;

    let opts = write_opts();
    let req = WriteRequest {
        domain: "example.com",
        name: "www",
        record_type: "A",
        value: "1.2.3.4",
        opts: &opts,
        replace_conflicting: true,
        replacing: None,
        debug: false,
    };
    let outcomes = write_with_conflict_handling(&client_for(&server), &req).await;

    delete.assert_async().await;
    // delete_conflicts' outcome must come first, then the retry outcome —
    // the order `summarize_set_outcomes`'s breakdown reports to the user.
    assert_eq!(outcomes.len(), 2);
    assert_eq!(outcomes[0].kind, "deleted");
    assert_eq!(outcomes[0].detail, "CNAME parkpage.godaddy.com");
    assert!(outcomes[0].error.is_none(), "{:?}", outcomes[0].error);
    assert_eq!(outcomes[1].kind, "created");
    assert_eq!(outcomes[1].detail, "1.2.3.4");
    // The retry hits the same mocked conflict response; unlike the first
    // attempt, a failure here is reported generically (no re-diagnosis).
    let err = outcomes[1].error.as_ref().expect("retry still failed");
    assert!(err.contains("Duplicate data provided"), "{err}");
    // A retry that is still a duplicate is `RECORD_EXISTS`, like the first try.
    assert_eq!(err.kind(), FailureKind::RecordExists);
}

// --- apply_replace -------------------------------------------------------
//
// `set`'s "replace" is one in-place `PUT` of the paired record. These exercise:
// the happy path (a single PUT, one "replaced" outcome, no create or delete),
// the PUT failing, and the conflict handling a replace shares with a create.
// Pairings that need no change never reach `apply_replace`; the plan sets them
// aside (see `plan_set`).

fn replace_req<'a>(opts: &'a RecordOptions, value: &'a str) -> WriteRequest<'a> {
    WriteRequest {
        domain: "example.com",
        name: "www",
        record_type: "A",
        value,
        opts,
        replace_conflicting: false,
        replacing: None,
        debug: false,
    }
}

const RECORD_PATH: &str = "/v3/domains/zones/example.com/dns-records";

/// Mocks that fail the test if a replace falls back to a create or delete.
async fn no_create_or_delete<'a>(
    server: &'a MockServer,
) -> (httpmock::Mock<'a>, httpmock::Mock<'a>) {
    let create = server
        .mock_async(|when, then| {
            when.method(POST).path(RECORD_PATH);
            then.status(201).json_body(json!({}));
        })
        .await;
    let delete = server
        .mock_async(|when, then| {
            when.method(DELETE).path(format!("{RECORD_PATH}/old-1"));
            then.status(204);
        })
        .await;
    (create, delete)
}

#[tokio::test]
async fn apply_replace_puts_the_new_value_onto_the_old_record() {
    let server = MockServer::start_async().await;
    let put = server
        .mock_async(|when, then| {
            when.method(PUT)
                .path(format!("{RECORD_PATH}/old-1"))
                .json_body(json!({ "type": "A", "name": "www", "data": "9.9.9.9", "ttl": 3600 }));
            then.status(200).json_body(json!({
                "recordId": "old-1", "type": "A", "name": "www", "data": "9.9.9.9", "ttl": 3600
            }));
        })
        .await;
    let (create, delete) = no_create_or_delete(&server).await;

    let opts = write_opts();
    let req = replace_req(&opts, "9.9.9.9");
    let outcomes = apply_replace(&client_for(&server), &req, "old-1", Some(3600)).await;

    put.assert_async().await;
    assert_eq!(create.calls_async().await, 0, "a replace must not create");
    assert_eq!(delete.calls_async().await, 0, "a replace must not delete");
    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].kind, "replaced");
    assert!(outcomes[0].error.is_none(), "{:?}", outcomes[0].error);
}

/// A changed TTL on an otherwise identical record is one `PUT` — no
/// delete-then-create, and no window in which the value is missing.
#[tokio::test]
async fn apply_replace_changes_only_the_ttl_with_a_single_put() {
    let server = MockServer::start_async().await;
    let put = server
        .mock_async(|when, then| {
            when.method(PUT)
                .path(format!("{RECORD_PATH}/old-1"))
                .json_body(json!({ "type": "A", "name": "www", "data": "1.2.3.4", "ttl": 7200 }));
            then.status(200).json_body(json!({
                "recordId": "old-1", "type": "A", "name": "www", "data": "1.2.3.4", "ttl": 7200
            }));
        })
        .await;
    let (create, delete) = no_create_or_delete(&server).await;

    let mut opts = write_opts();
    opts.ttl = Some(7200);
    let req = replace_req(&opts, "1.2.3.4");
    let outcomes = apply_replace(&client_for(&server), &req, "old-1", Some(3600)).await;

    put.assert_async().await;
    assert_eq!(create.calls_async().await, 0);
    assert_eq!(delete.calls_async().await, 0);
    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].kind, "replaced");
    assert!(outcomes[0].error.is_none(), "{:?}", outcomes[0].error);
}

#[tokio::test]
async fn apply_replace_sends_the_changed_tlsa_fields_in_the_put_body() {
    let server = MockServer::start_async().await;
    let cert = "d2abde240d7cd3ee6b4b28c54df034b97983a1d16e8a410e4561cb106618e971";
    let put = server
        .mock_async(|when, then| {
            when.method(PUT)
                .path(format!("{RECORD_PATH}/old-1"))
                .json_body_includes(
                    json!({ "usage": 1, "selector": 0, "matchingType": 0 }).to_string(),
                );
            then.status(200).json_body(json!({
                "recordId": "old-1",
                "type": "TLSA",
                "name": "www",
                "ttl": 3600,
                "usage": 1,
                "selector": 0,
                "matchingType": 0,
                "certificateData": cert
            }));
        })
        .await;

    let mut opts = write_opts();
    opts.usage = Some(1);
    opts.selector = Some(0);
    opts.matching_type = Some(0);
    let req = WriteRequest {
        record_type: "TLSA",
        value: cert,
        opts: &opts,
        ..replace_req(&opts, cert)
    };
    let outcomes = apply_replace(&client_for(&server), &req, "old-1", Some(3600)).await;

    put.assert_async().await;
    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].kind, "replaced");
    assert!(outcomes[0].error.is_none(), "{:?}", outcomes[0].error);
}

#[tokio::test]
async fn apply_replace_reports_the_failure_and_touches_nothing_else_when_the_put_fails() {
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(PUT).path(format!("{RECORD_PATH}/old-1"));
            then.status(500);
        })
        .await;
    let (create, delete) = no_create_or_delete(&server).await;

    let opts = write_opts();
    let req = replace_req(&opts, "9.9.9.9");
    let outcomes = apply_replace(&client_for(&server), &req, "old-1", Some(3600)).await;

    assert_eq!(create.calls_async().await, 0);
    assert_eq!(delete.calls_async().await, 0);
    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].kind, "replaced");
    assert!(outcomes[0].error.is_some());
}

/// A replace that collides with a CNAME at the name gets the same
/// `--replace-conflicting-types` handling a create does: delete the
/// conflicting record, then retry the PUT once.
#[tokio::test]
async fn apply_replace_with_the_flag_deletes_a_conflicting_cname_then_retries_the_put() {
    let server = MockServer::start_async().await;
    let first_put = server
        .mock_async(|when, then| {
            when.method(PUT).path(format!("{RECORD_PATH}/old-1"));
            then.status(422).body(DUPLICATE_RECORD_BODY);
        })
        .await;
    server
        .mock_async(|when, then| {
            when.method(GET).path(RECORD_PATH);
            then.status(200).json_body(json!({
                "items": [
                    { "recordId": "cn-1", "type": "CNAME", "name": "www", "data": "parkpage.godaddy.com", "ttl": 3600 },
                    { "recordId": "old-1", "type": "AAAA", "name": "www", "data": "::1", "ttl": 3600 }
                ],
                "links": []
            }));
        })
        .await;
    let delete_conflict = server
        .mock_async(|when, then| {
            when.method(DELETE).path(format!("{RECORD_PATH}/cn-1"));
            then.status(204);
        })
        .await;

    let opts = write_opts();
    let req = WriteRequest {
        replace_conflicting: true,
        ..replace_req(&opts, "1.2.3.4")
    };
    let outcomes = apply_replace(&client_for(&server), &req, "old-1", Some(3600)).await;

    // The retried PUT hits the same mocked conflict, so it reports failure, but
    // the conflicting CNAME was deleted first and the PUT attempted twice.
    delete_conflict.assert_async().await;
    assert_eq!(first_put.calls_async().await, 2);
    assert_eq!(outcomes.len(), 2);
    assert_eq!(outcomes[0].kind, "deleted");
    assert_eq!(outcomes[1].kind, "replaced");
}

/// With no `--ttl`, a replaced record keeps the TTL it had instead of being
/// reset to the 3600 default.
#[tokio::test]
async fn apply_replace_keeps_the_stored_ttl_when_none_is_given() {
    let server = MockServer::start_async().await;
    let put = server
        .mock_async(|when, then| {
            when.method(PUT)
                .path(format!("{RECORD_PATH}/old-1"))
                .json_body(json!({ "type": "A", "name": "www", "data": "9.9.9.9", "ttl": 600 }));
            then.status(200).json_body(json!({
                "recordId": "old-1", "type": "A", "name": "www", "data": "9.9.9.9", "ttl": 600
            }));
        })
        .await;

    let opts = write_opts();
    let req = replace_req(&opts, "9.9.9.9");
    let outcomes = apply_replace(&client_for(&server), &req, "old-1", Some(600)).await;

    put.assert_async().await;
    assert!(outcomes[0].error.is_none(), "{:?}", outcomes[0].error);
}

/// An explicit `--ttl` wins over the stored one.
#[tokio::test]
async fn apply_replace_uses_an_explicit_ttl_over_the_stored_one() {
    let server = MockServer::start_async().await;
    let put = server
        .mock_async(|when, then| {
            when.method(PUT)
                .path(format!("{RECORD_PATH}/old-1"))
                .json_body(json!({ "type": "A", "name": "www", "data": "9.9.9.9", "ttl": 900 }));
            then.status(200).json_body(json!({
                "recordId": "old-1", "type": "A", "name": "www", "data": "9.9.9.9", "ttl": 900
            }));
        })
        .await;

    let mut opts = write_opts();
    opts.ttl = Some(900);
    let req = replace_req(&opts, "9.9.9.9");
    let outcomes = apply_replace(&client_for(&server), &req, "old-1", Some(600)).await;

    put.assert_async().await;
    assert!(outcomes[0].error.is_none(), "{:?}", outcomes[0].error);
}

/// A failed write's kind comes from what the API said, so the command's error
/// code reflects it: an exact duplicate is `RECORD_EXISTS`, a missing zone is
/// `ZONE_NOT_FOUND`, anything else stays `Other`.
#[tokio::test]
async fn a_failed_write_is_classified_by_what_the_api_said() {
    use crate::dns::failure::FailureKind;

    for (status, body, want) in [
        (
            404,
            json!({"name":"ZONE_NOT_FOUND","message":"zone not found"}),
            FailureKind::ZoneNotFound,
        ),
        (
            422,
            json!({"name":"VALIDATION_ERROR","message":"Request failed validation"}),
            FailureKind::Validation,
        ),
        (500, json!({"name":"INTERNAL"}), FailureKind::Other),
    ] {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v3/domains/zones/example.com/dns-records");
                then.status(status).json_body(body.clone());
            })
            .await;
        let opts = write_opts();
        let req = WriteRequest {
            domain: "example.com",
            name: "www",
            record_type: "A",
            value: "1.2.3.4",
            opts: &opts,
            replace_conflicting: false,
            replacing: None,
            debug: false,
        };
        let outcomes = write_with_conflict_handling(&client_for(&server), &req).await;
        let failure = outcomes[0].error.as_ref().expect("the write failed");
        assert_eq!(failure.kind(), want, "{status} {body}");
    }
}

#[tokio::test]
async fn an_exact_duplicate_is_a_record_exists_failure() {
    use crate::dns::failure::FailureKind;

    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/v3/domains/zones/example.com/dns-records");
            then.status(422).json_body(
                serde_json::from_str::<Value>(DUPLICATE_RECORD_BODY).expect("valid json"),
            );
        })
        .await;
    server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/v3/domains/zones/example.com/dns-records")
                .query_param("name", "www");
            then.status(200).json_body(json!({
                "items": [
                    { "type": "A", "name": "www", "data": "1.2.3.4", "ttl": 3600, "recordId": "a-1" }
                ],
                "totalItems": 1, "totalPages": 1, "links": []
            }));
        })
        .await;
    let opts = write_opts();
    let req = WriteRequest {
        domain: "example.com",
        name: "www",
        record_type: "A",
        value: "1.2.3.4",
        opts: &opts,
        replace_conflicting: false,
        replacing: None,
        debug: false,
    };
    let outcomes = write_with_conflict_handling(&client_for(&server), &req).await;
    let failure = outcomes[0].error.as_ref().expect("duplicate reported");
    assert_eq!(failure.kind(), FailureKind::RecordExists);
    assert!(failure.contains("exact value already exists"), "{failure}");
}

/// The conflict lookup only explains a duplicate. When it fails, the write's own
/// error is what gets reported, still as `RECORD_EXISTS`: the code and the message
/// describe the same failure.
#[tokio::test]
async fn a_failed_conflict_lookup_keeps_the_writes_own_duplicate_error() {
    use crate::dns::failure::FailureKind;

    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/v3/domains/zones/example.com/dns-records");
            then.status(422).json_body(
                serde_json::from_str::<Value>(DUPLICATE_RECORD_BODY).expect("valid json"),
            );
        })
        .await;
    server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/v3/domains/zones/example.com/dns-records");
            then.status(500).json_body(json!({"name": "INTERNAL"}));
        })
        .await;
    let opts = write_opts();
    let req = WriteRequest {
        domain: "example.com",
        name: "www",
        record_type: "A",
        value: "1.2.3.4",
        opts: &opts,
        replace_conflicting: false,
        replacing: None,
        debug: false,
    };
    let outcomes = write_with_conflict_handling(&client_for(&server), &req).await;
    let failure = outcomes[0].error.as_ref().expect("the write failed");
    assert_eq!(failure.kind(), FailureKind::RecordExists);
    assert!(failure.contains("Duplicate data provided"), "{failure}");
    assert!(!failure.contains("INTERNAL"), "{failure}");
}
