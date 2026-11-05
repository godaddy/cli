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
            then.status(201)
                .json_body(json!({ "type": "A", "name": "www", "data": "1.2.3.4", "ttl": 3600 }));
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
        debug: false,
    };
    let outcomes = write_with_conflict_handling(&client_for(&server), &req).await;

    create.assert_async().await;
    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].kind, "created");
    assert!(outcomes[0].error.is_none(), "{:?}", outcomes[0].error);
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
    let err = outcomes[1].error.as_deref().expect("retry still failed");
    assert!(err.contains("Duplicate data provided"), "{err}");
}

// --- apply_replace -------------------------------------------------------
//
// `set`'s "replace" semantic is create-the-new-value-then-delete-the-old
// (v3 has no atomic single-record replace; see GH #136). These exercise:
// the happy path (create then delete, one "replaced" outcome), the create
// failing (delete must never be attempted — the old record stays intact),
// and the delete failing after a successful create (reported as a
// separate "cleanup" outcome that still fails the command).

fn replace_req<'a>(opts: &'a RecordOptions, value: &'a str) -> WriteRequest<'a> {
    WriteRequest {
        domain: "example.com",
        name: "www",
        record_type: "A",
        value,
        opts,
        replace_conflicting: false,
        debug: false,
    }
}

fn old_a_record(data: &str) -> types::DnsRecord {
    types::DnsRecord {
        certificate_data: None,
        matching_type: None,
        selector: None,
        usage: None,
        data: Some(data.to_owned()),
        flag: None,
        name: "www".to_owned(),
        parameters: None,
        port: None,
        priority: None,
        protocol: None,
        record_id: Some("old-1".to_owned()),
        service: None,
        tag: None,
        ttl: 3600,
        type_: types::DnsRecordType("A".to_owned()),
        weight: None,
    }
}

#[tokio::test]
async fn apply_replace_creates_then_deletes_the_old_record() {
    let server = MockServer::start_async().await;
    let create = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/v3/domains/zones/example.com/dns-records");
            then.status(201)
                .json_body(json!({ "type": "A", "name": "www", "data": "9.9.9.9", "ttl": 3600 }));
        })
        .await;
    let delete = server
        .mock_async(|when, then| {
            when.method(DELETE)
                .path("/v3/domains/zones/example.com/dns-records/old-1");
            then.status(204);
        })
        .await;

    let opts = write_opts();
    let req = replace_req(&opts, "9.9.9.9");
    let old_record = old_a_record("1.2.3.4");
    let outcomes = apply_replace(
        &client_for(&server),
        &req,
        "old-1",
        "A 1.2.3.4",
        Some(&old_record),
    )
    .await;

    create.assert_async().await;
    delete.assert_async().await;
    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].kind, "replaced");
    assert!(outcomes[0].error.is_none(), "{:?}", outcomes[0].error);
}

/// The record being "replaced" already holds the desired value (e.g. a
/// re-run of the same `set`, or only some of several `--data` values
/// actually changed) — creating it again would hit v3's exact-duplicate
/// `DUPLICATE_RECORD` since the old record hasn't been deleted yet, so
/// this must short-circuit as a no-op success without any API calls.
#[tokio::test]
async fn apply_replace_is_a_no_op_when_old_record_already_has_the_desired_value() {
    let server = MockServer::start_async().await;
    let create = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/v3/domains/zones/example.com/dns-records");
            then.status(201);
        })
        .await;
    let delete = server
        .mock_async(|when, then| {
            when.method(DELETE)
                .path("/v3/domains/zones/example.com/dns-records/old-1");
            then.status(204);
        })
        .await;

    let opts = write_opts();
    let req = replace_req(&opts, "1.2.3.4");
    let old_record = old_a_record("1.2.3.4");
    let outcomes = apply_replace(
        &client_for(&server),
        &req,
        "old-1",
        "A 1.2.3.4",
        Some(&old_record),
    )
    .await;

    assert_eq!(
        create.calls_async().await,
        0,
        "no create for a no-op replace"
    );
    assert_eq!(
        delete.calls_async().await,
        0,
        "no delete for a no-op replace"
    );
    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].kind, "replaced");
    assert_eq!(outcomes[0].detail, "1.2.3.4");
    assert!(outcomes[0].error.is_none(), "{:?}", outcomes[0].error);
}

/// `same_content` deliberately ignores `ttl`, so an explicit `--ttl`
/// change has to be checked separately — otherwise a `dns set` that only
/// changes the TTL would be silently skipped as a no-op.
#[tokio::test]
async fn apply_replace_is_not_a_no_op_when_ttl_explicitly_changes() {
    let server = MockServer::start_async().await;
    let create = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/v3/domains/zones/example.com/dns-records");
            then.status(201)
                .json_body(json!({ "type": "A", "name": "www", "data": "1.2.3.4", "ttl": 7200 }));
        })
        .await;
    let delete = server
        .mock_async(|when, then| {
            when.method(DELETE)
                .path("/v3/domains/zones/example.com/dns-records/old-1");
            then.status(204);
        })
        .await;

    let mut opts = write_opts();
    opts.ttl = Some(7200);
    let req = replace_req(&opts, "1.2.3.4");
    let old_record = old_a_record("1.2.3.4"); // ttl: 3600
    let outcomes = apply_replace(
        &client_for(&server),
        &req,
        "old-1",
        "A 1.2.3.4",
        Some(&old_record),
    )
    .await;

    create.assert_async().await;
    delete.assert_async().await;
    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].kind, "replaced");
    assert!(outcomes[0].error.is_none(), "{:?}", outcomes[0].error);
}

/// Omitting `--ttl` must not force a replace just because the CLI's
/// default (3600) differs from the existing record's actual TTL — only
/// an *explicit* `--ttl` counts as a real change.
#[tokio::test]
async fn apply_replace_is_still_a_no_op_when_ttl_is_omitted_even_if_existing_ttl_differs() {
    let server = MockServer::start_async().await;
    let create = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/v3/domains/zones/example.com/dns-records");
            then.status(201);
        })
        .await;
    let delete = server
        .mock_async(|when, then| {
            when.method(DELETE)
                .path("/v3/domains/zones/example.com/dns-records/old-1");
            then.status(204);
        })
        .await;

    let opts = write_opts(); // ttl: None — no explicit --ttl
    let req = replace_req(&opts, "1.2.3.4");
    let old_record = types::DnsRecord {
        ttl: 7200, // differs from v3_record's DEFAULT_TTL fallback (3600)
        ..old_a_record("1.2.3.4")
    };
    let outcomes = apply_replace(
        &client_for(&server),
        &req,
        "old-1",
        "A 1.2.3.4",
        Some(&old_record),
    )
    .await;

    assert_eq!(
        create.calls_async().await,
        0,
        "no create for a no-op replace"
    );
    assert_eq!(
        delete.calls_async().await,
        0,
        "no delete for a no-op replace"
    );
    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].kind, "replaced");
    assert!(outcomes[0].error.is_none(), "{:?}", outcomes[0].error);
}

/// A TLSA record's identity spans `usage`/`selector`/`matchingType`, not
/// just its certificate data — keeping the same `--data` while changing
/// `--usage` is a real change, not a no-op, even though comparing only
/// the value string would say otherwise.
#[tokio::test]
async fn apply_replace_is_not_a_no_op_when_only_a_sibling_tlsa_field_changes() {
    let server = MockServer::start_async().await;
    let cert = "d2abde240d7cd3ee6b4b28c54df034b97983a1d16e8a410e4561cb106618e971";
    let create = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/v3/domains/zones/example.com/dns-records");
            then.status(201).json_body(json!({
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
    let delete = server
        .mock_async(|when, then| {
            when.method(DELETE)
                .path("/v3/domains/zones/example.com/dns-records/old-1");
            then.status(204);
        })
        .await;

    let mut opts = write_opts();
    opts.usage = Some(1);
    opts.selector = Some(0);
    opts.matching_type = Some(0);
    let req = WriteRequest {
        domain: "example.com",
        name: "www",
        record_type: "TLSA",
        value: cert,
        opts: &opts,
        replace_conflicting: false,
        debug: false,
    };
    let old_record = types::DnsRecord {
        certificate_data: Some(cert.to_owned()),
        matching_type: Some(types::TlsaMatchingType(1)),
        selector: Some(types::TlsaSelector(1)),
        usage: Some(types::TlsaUsage(3)),
        data: None,
        flag: None,
        name: "www".to_owned(),
        parameters: None,
        port: None,
        priority: None,
        protocol: None,
        record_id: Some("old-1".to_owned()),
        service: None,
        tag: None,
        ttl: 3600,
        type_: types::DnsRecordType("TLSA".to_owned()),
        weight: None,
    };
    let outcomes = apply_replace(
        &client_for(&server),
        &req,
        "old-1",
        "TLSA 3 1 1 ...",
        Some(&old_record),
    )
    .await;

    create.assert_async().await;
    delete.assert_async().await;
    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].kind, "replaced");
    assert!(outcomes[0].error.is_none(), "{:?}", outcomes[0].error);
}

#[tokio::test]
async fn apply_replace_leaves_old_record_alone_when_create_fails() {
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/v3/domains/zones/example.com/dns-records");
            then.status(500);
        })
        .await;
    let delete = server
        .mock_async(|when, then| {
            when.method(DELETE)
                .path("/v3/domains/zones/example.com/dns-records/old-1");
            then.status(204);
        })
        .await;

    let opts = write_opts();
    let req = replace_req(&opts, "9.9.9.9");
    let old_record = old_a_record("1.2.3.4");
    let outcomes = apply_replace(
        &client_for(&server),
        &req,
        "old-1",
        "A 1.2.3.4",
        Some(&old_record),
    )
    .await;

    assert_eq!(
        delete.calls_async().await,
        0,
        "the old record must not be touched when the create fails"
    );
    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].kind, "replaced");
    assert!(outcomes[0].error.is_some());
}

// --- retime_record (value kept, explicit --ttl differs) -------------------
//
// The old record is 1.2.3.4 @ ttl 3600; the set asks for 1.2.3.4 @ ttl 7200.
// The new-TTL create and the put-back of the old record are told apart by the
// `ttl` in the request body.

fn retime_opts() -> RecordOptions {
    let mut opts = write_opts();
    opts.ttl = Some(7200);
    opts
}

#[tokio::test]
async fn retime_does_not_create_when_the_old_record_cannot_be_deleted() {
    let server = MockServer::start_async().await;
    let create = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/v3/domains/zones/example.com/dns-records");
            then.status(201);
        })
        .await;
    server
        .mock_async(|when, then| {
            when.method(DELETE)
                .path("/v3/domains/zones/example.com/dns-records/old-1");
            then.status(500);
        })
        .await;

    let opts = retime_opts();
    let req = replace_req(&opts, "1.2.3.4");
    let old_record = old_a_record("1.2.3.4");
    let outcomes = apply_replace(
        &client_for(&server),
        &req,
        "old-1",
        "A 1.2.3.4",
        Some(&old_record),
    )
    .await;

    // Delete-first is what avoids the duplicate create, so a failed delete
    // has to stop everything before a create is attempted.
    assert_eq!(create.calls_async().await, 0);
    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].kind, "replaced");
    assert!(outcomes[0].error.is_some());
}

#[tokio::test]
async fn retime_puts_the_old_record_back_when_the_new_ttl_cannot_be_created() {
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/v3/domains/zones/example.com/dns-records")
                .json_body_includes(r#"{"ttl":7200}"#);
            then.status(500);
        })
        .await;
    let restore = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/v3/domains/zones/example.com/dns-records")
                .json_body_includes(r#"{"ttl":3600}"#);
            then.status(201)
                .json_body(json!({ "type": "A", "name": "www", "data": "1.2.3.4", "ttl": 3600 }));
        })
        .await;
    let delete = server
        .mock_async(|when, then| {
            when.method(DELETE)
                .path("/v3/domains/zones/example.com/dns-records/old-1");
            then.status(204);
        })
        .await;

    let opts = retime_opts();
    let req = replace_req(&opts, "1.2.3.4");
    let old_record = old_a_record("1.2.3.4");
    let outcomes = apply_replace(
        &client_for(&server),
        &req,
        "old-1",
        "A 1.2.3.4",
        Some(&old_record),
    )
    .await;

    delete.assert_async().await;
    restore.assert_async().await;
    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].kind, "replaced");
    let err = outcomes[0]
        .error
        .as_deref()
        .expect("create failure reported");
    assert!(err.contains("left as it was"), "{err}");
}

#[tokio::test]
async fn retime_reports_cleanup_when_the_old_record_cannot_be_put_back() {
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/v3/domains/zones/example.com/dns-records");
            then.status(500);
        })
        .await;
    server
        .mock_async(|when, then| {
            when.method(DELETE)
                .path("/v3/domains/zones/example.com/dns-records/old-1");
            then.status(204);
        })
        .await;

    let opts = retime_opts();
    let req = replace_req(&opts, "1.2.3.4");
    let old_record = old_a_record("1.2.3.4");
    let outcomes = apply_replace(
        &client_for(&server),
        &req,
        "old-1",
        "A 1.2.3.4",
        Some(&old_record),
    )
    .await;

    assert_eq!(outcomes.len(), 2);
    assert_eq!(outcomes[0].kind, "replaced");
    assert!(outcomes[0].error.is_some());
    assert_eq!(outcomes[1].kind, "cleanup");
    let err = outcomes[1]
        .error
        .as_deref()
        .expect("restore failure reported");
    assert!(err.contains("putting it back failed"), "{err}");
}

#[tokio::test]
async fn apply_replace_reports_cleanup_outcome_when_delete_of_old_record_fails() {
    let server = MockServer::start_async().await;
    let create = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/v3/domains/zones/example.com/dns-records");
            then.status(201)
                .json_body(json!({ "type": "A", "name": "www", "data": "9.9.9.9", "ttl": 3600 }));
        })
        .await;
    let delete = server
        .mock_async(|when, then| {
            when.method(DELETE)
                .path("/v3/domains/zones/example.com/dns-records/old-1");
            then.status(404).json_body(json!({
                "correlationId": "c-1",
                "message": "Record not found in DNS zone.",
                "name": "RECORD_NOT_FOUND"
            }));
        })
        .await;

    let opts = write_opts();
    let req = replace_req(&opts, "9.9.9.9");
    let old_record = old_a_record("1.2.3.4");
    let outcomes = apply_replace(
        &client_for(&server),
        &req,
        "old-1",
        "A 1.2.3.4",
        Some(&old_record),
    )
    .await;

    create.assert_async().await;
    delete.assert_async().await;
    assert_eq!(outcomes.len(), 2);
    assert_eq!(outcomes[0].kind, "replaced");
    assert!(outcomes[0].error.is_none(), "{:?}", outcomes[0].error);
    assert_eq!(outcomes[1].kind, "cleanup");
    let err = outcomes[1]
        .error
        .as_deref()
        .expect("cleanup failure reported");
    assert!(err.contains("A 1.2.3.4"), "{err}");
    assert!(err.contains("dns list"), "{err}");
}
