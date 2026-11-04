use super::*;
use crate::dns::records::{RecordOptions, v3_record};
use httpmock::prelude::*;
use serde_json::json;

fn opts() -> RecordOptions {
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

fn a(value: &str) -> types::DnsRecord {
    v3_record("www", "A", value, &opts())
}

fn stored(mut record: types::DnsRecord, id: &str) -> types::DnsRecord {
    record.record_id = Some(id.to_owned());
    record
}

#[test]
fn matching_sets_have_no_mismatch_whatever_their_order_or_ids() {
    let desired = [a("1.1.1.1"), a("2.2.2.2")];
    let actual = [stored(a("2.2.2.2"), "r2"), stored(a("1.1.1.1"), "r1")];
    assert_eq!(final_set_mismatch(&desired, &actual, false), None);
}

#[test]
fn a_record_added_by_someone_else_is_reported_as_unexpected() {
    let desired = [a("192.0.2.10")];
    let actual = [
        stored(a("198.51.100.77"), "r9"),
        stored(a("192.0.2.10"), "r1"),
    ];
    assert_eq!(
        final_set_mismatch(&desired, &actual, false),
        Some(Mismatch {
            missing: vec![],
            unexpected: vec!["A 198.51.100.77".to_owned()],
        })
    );
}

#[test]
fn a_requested_record_that_is_gone_is_reported_as_missing() {
    let desired = [a("1.1.1.1"), a("2.2.2.2")];
    let actual = [stored(a("1.1.1.1"), "r1")];
    assert_eq!(
        final_set_mismatch(&desired, &actual, false),
        Some(Mismatch {
            missing: vec!["A 2.2.2.2".to_owned()],
            unexpected: vec![],
        })
    );
}

#[test]
fn duplicates_are_counted_not_collapsed() {
    let desired = [a("1.1.1.1")];
    let actual = [stored(a("1.1.1.1"), "r1"), stored(a("1.1.1.1"), "r2")];
    let mismatch = final_set_mismatch(&desired, &actual, false).expect("one extra");
    assert_eq!(mismatch.unexpected, vec!["A 1.1.1.1".to_owned()]);
}

#[test]
fn ttl_is_compared_only_when_the_caller_set_one() {
    let mut slow = opts();
    slow.ttl = Some(900);
    let desired = [v3_record("www", "A", "1.1.1.1", &slow)];
    let actual = [stored(a("1.1.1.1"), "r1")]; // stored with the default 3600

    assert_eq!(final_set_mismatch(&desired, &actual, false), None);
    assert!(final_set_mismatch(&desired, &actual, true).is_some());
}

#[test]
fn names_are_compared_the_way_the_api_stores_them() {
    // `Example.COM` is stored as `example.com.`.
    let desired = [v3_record("www", "CNAME", "Example.COM", &opts())];
    let actual = [stored(
        v3_record("www", "CNAME", "example.com.", &opts()),
        "r1",
    )];
    assert_eq!(final_set_mismatch(&desired, &actual, false), None);
}

#[test]
fn txt_text_is_compared_exactly() {
    let desired = [v3_record("www", "TXT", "Hello", &opts())];
    let actual = [stored(v3_record("www", "TXT", "hello", &opts()), "r1")];
    assert!(final_set_mismatch(&desired, &actual, false).is_some());
}

#[test]
fn records_that_differ_only_in_a_sibling_field_are_different() {
    let mut p10 = opts();
    p10.priority = Some(10);
    let mut p20 = opts();
    p20.priority = Some(20);
    let desired = [v3_record("@", "MX", "mail.example.com.", &p10)];
    let actual = [stored(
        v3_record("@", "MX", "mail.example.com.", &p20),
        "r1",
    )];
    let mismatch = final_set_mismatch(&desired, &actual, false).expect("priority differs");
    assert_eq!(
        mismatch.missing,
        vec!["MX priority 10 mail.example.com.".to_owned()]
    );
    assert_eq!(
        mismatch.unexpected,
        vec!["MX priority 20 mail.example.com.".to_owned()]
    );
}

fn client_for(server: &MockServer) -> domains_client::Client {
    domains_client::client_with_builder(
        reqwest::Client::builder(),
        &server.base_url(),
        "Bearer tok",
        "req-1",
    )
    .expect("build client")
}

fn serve_records(server: &MockServer, items: Value) {
    server.mock(|when, then| {
        when.method(GET)
            .path("/v3/domains/zones/example.com/dns-records")
            .query_param("type", "A")
            .query_param("name", "www");
        then.status(200)
            .json_body(json!({ "items": items, "links": [] }));
    });
}

#[tokio::test]
async fn verify_passes_when_the_zone_holds_exactly_the_requested_records() {
    let server = MockServer::start_async().await;
    serve_records(
        &server,
        json!([{ "recordId": "r1", "type": "A", "name": "www", "data": "1.1.1.1", "ttl": 3600 }]),
    );
    verify_final_state(
        &client_for(&server),
        "example.com",
        "A",
        "www",
        &[a("1.1.1.1")],
        false,
        false,
    )
    .await
    .expect("zone matches");
}

#[tokio::test]
async fn verify_fails_without_deleting_when_an_unrequested_record_appeared() {
    let server = MockServer::start_async().await;
    serve_records(
        &server,
        json!([
            { "recordId": "r1", "type": "A", "name": "www", "data": "1.1.1.1", "ttl": 3600 },
            { "recordId": "r9", "type": "A", "name": "www", "data": "198.51.100.77", "ttl": 3600 }
        ]),
    );
    let delete = server.mock(|when, then| {
        when.method(DELETE);
        then.status(204);
    });

    let err = verify_final_state(
        &client_for(&server),
        "example.com",
        "A",
        "www",
        &[a("1.1.1.1")],
        false,
        false,
    )
    .await
    .expect_err("an extra record is a mismatch");

    assert_eq!(delete.calls(), 0, "an unexpected record must be left alone");
    let text = format!("{err:?}");
    assert!(text.contains("Present but not requested"), "{text}");
    assert!(text.contains("A 198.51.100.77"), "{text}");
    assert!(
        !text.contains("Requested but not present"),
        "nothing requested is missing: {text}"
    );
}

#[tokio::test]
async fn verify_reports_a_failed_read_back_as_applied_but_unconfirmed() {
    let server = MockServer::start_async().await;
    server.mock(|when, then| {
        when.method(GET);
        then.status(500);
    });
    let err = verify_final_state(
        &client_for(&server),
        "example.com",
        "A",
        "www",
        &[a("1.1.1.1")],
        false,
        false,
    )
    .await
    .expect_err("read-back failed");
    let text = format!("{err:?}");
    assert!(text.contains("the change was applied"), "{text}");
}
