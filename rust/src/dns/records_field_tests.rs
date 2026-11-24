//! Tests for what `dns add` and `dns set` accept for a record type: flags the type
//! does not use are rejected, and a missing zone is reported with its own code.

use super::tests::{client_for, opts};
use super::*;
use httpmock::prelude::*;
use serde_json::json;

#[test]
fn flags_a_record_type_does_not_use_are_rejected_not_dropped() {
    // The DEVEX-1384 case: `--priority` on a TXT record used to be accepted and
    // silently left out of the record.
    let mut txt = opts();
    txt.priority = Some(5);
    let err = validate_applicable_fields("TXT", &txt).expect_err("priority is not for TXT");
    assert_eq!(
        err,
        "--priority is only valid for MX, SRV, HTTPS, and SVCB records, not TXT"
    );

    let cases = [
        (
            "A",
            RecordOptions {
                port: Some(80),
                ..opts()
            },
            "--port",
        ),
        (
            "MX",
            RecordOptions {
                weight: Some(1),
                ..opts()
            },
            "--weight",
        ),
        (
            "CNAME",
            RecordOptions {
                service: Some("_sip".into()),
                ..opts()
            },
            "--service",
        ),
        (
            "MX",
            RecordOptions {
                protocol: Some("_tcp".into()),
                ..opts()
            },
            "--protocol",
        ),
    ];
    for (record_type, given, flag) in cases {
        let err = validate_applicable_fields(record_type, &given).expect_err(flag);
        assert!(err.starts_with(flag), "{err}");
        assert!(err.ends_with(&format!("not {record_type}")), "{err}");
    }
}

#[test]
fn each_type_accepts_the_flags_it_uses() {
    let mut srv = opts();
    srv.priority = Some(1);
    srv.weight = Some(1);
    srv.port = Some(5060);
    srv.service = Some("_sip".into());
    srv.protocol = Some("_tcp".into());
    assert!(validate_applicable_fields("SRV", &srv).is_ok());

    let mut mx = opts();
    mx.priority = Some(10);
    assert!(validate_applicable_fields("MX", &mx).is_ok());
    assert!(validate_applicable_fields("HTTPS", &mx).is_ok());
    assert!(validate_applicable_fields("SVCB", &mx).is_ok());

    let mut tlsa = opts();
    tlsa.port = Some(443);
    tlsa.protocol = Some("_tcp".into());
    assert!(validate_applicable_fields("TLSA", &tlsa).is_ok());
    assert!(validate_applicable_fields("TXT", &opts()).is_ok());
}

/// A zone the API doesn't know comes back with its own stable code, not a bare
/// `ERROR` with the API's JSON in the message.
#[tokio::test]
async fn a_missing_zone_is_reported_as_zone_not_found() {
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/v3/domains/zones/nope.example/dns-records");
            then.status(404).json_body(json!({
                "correlationId": "c-1", "message": "zone not found", "name": "ZONE_NOT_FOUND"
            }));
        })
        .await;

    let err = fetch_records(&client_for(&server), "nope.example", None, None, false)
        .await
        .expect_err("the zone is missing");
    let envelope = cli_engine::build_error_envelope(&err, "domain");
    assert_eq!(
        envelope.error.as_ref().map(|e| e.code.as_str()),
        Some("ZONE_NOT_FOUND")
    );
    assert!(envelope.fix.is_some(), "{envelope:?}");
    // The message says what happened; the API's JSON is not the message.
    let msg = envelope.error.map(|e| e.message).unwrap_or_default();
    assert!(msg.contains("zone not found"), "{msg}");
}
