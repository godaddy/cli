use super::*;
use httpmock::prelude::*;
use serde_json::{Value, json};

fn client_for(server: &MockServer) -> domains_client::Client {
    domains_client::client_with_builder(
        reqwest::Client::builder(),
        &server.base_url(),
        "Bearer tok",
        "req-1",
    )
    .expect("build client")
}

fn srv(id: &str, name: &str, service: &str) -> Value {
    json!({
        "recordId": id, "type": "SRV", "name": name, "data": "t.example.com.", "ttl": 3600,
        "priority": 1, "weight": 1, "port": 5222, "service": service, "protocol": "_tcp"
    })
}

/// SRV is listed under its short name but filtered by its composed one, so
/// the name is matched here: no `name` goes to the API, and every record
/// whose listed name matches comes back, whatever its service.
#[tokio::test]
async fn fetch_records_matches_an_srv_name_client_side() {
    let server = MockServer::start_async().await;
    let list = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/v3/domains/zones/example.com/dns-records")
                .query_param("type", "SRV")
                .query_param_missing("name");
            then.status(200).json_body(json!({
                "items": [
                    srv("r1", "chat", "_xmpp"),
                    srv("r2", "Chat", "_xmpps"),
                    srv("r3", "other", "_sip")
                ],
                "links": []
            }));
        })
        .await;

    let found = fetch_records(
        &client_for(&server),
        "example.com",
        Some("SRV"),
        Some("chat"),
        false,
    )
    .await
    .expect("list");

    list.assert_async().await;
    let ids: Vec<_> = found
        .iter()
        .filter_map(|r| r.record_id.as_deref())
        .collect();
    assert_eq!(ids, ["r1", "r2"], "every service under the name, any case");
}

/// A selector written as the composed name the API understands finds the
/// record too, and so does a different case; a composed name for another
/// service does not.
#[tokio::test]
async fn fetch_records_accepts_a_composed_srv_or_tlsa_name() {
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/v3/domains/zones/example.com/dns-records")
                .query_param("type", "SRV");
            then.status(200).json_body(json!({
                "items": [srv("r1", "chat", "_xmpp"), srv("r2", "chat", "_sip")],
                "links": []
            }));
        })
        .await;
    let client = client_for(&server);
    let ids = |found: Vec<types::DnsRecord>| -> Vec<String> {
        found.into_iter().filter_map(|r| r.record_id).collect()
    };

    let one = fetch_records(
        &client,
        "example.com",
        Some("SRV"),
        Some("_XMPP._tcp.chat"),
        false,
    )
    .await
    .expect("list");
    assert_eq!(ids(one), ["r1"], "only the service the composed name names");
    let none = fetch_records(
        &client,
        "example.com",
        Some("SRV"),
        Some("_ftp._tcp.chat"),
        false,
    )
    .await
    .expect("list");
    assert!(none.is_empty());

    let tlsa = types::DnsRecord {
        type_: types::DnsRecordType("TLSA".to_owned()),
        port: Some(443),
        protocol: Some("_tcp".to_owned()),
        name: "www".to_owned(),
        ..serde_json::from_value(srv("t1", "www", "_x")).expect("record")
    };
    assert!(listed_name_matches(&tlsa, "_443._tcp.www"));
    assert!(listed_name_matches(&tlsa, "www"));
    assert!(!listed_name_matches(&tlsa, "_25._tcp.www"));
}

/// At the zone apex the composed name has no `.@` suffix: just the prefix.
#[test]
fn listed_name_matches_composes_apex_names_without_a_suffix() {
    let srv_apex: types::DnsRecord =
        serde_json::from_value(srv("r1", "@", "_sip")).expect("record");
    assert!(listed_name_matches(&srv_apex, "@"));
    assert!(listed_name_matches(&srv_apex, "_sip._tcp"));
    assert!(listed_name_matches(&srv_apex, "_SIP._TCP"));
    assert!(!listed_name_matches(&srv_apex, "_sip._tcp.@"));
    assert!(!listed_name_matches(&srv_apex, "_xmpp._tcp"));

    let tlsa_apex = types::DnsRecord {
        type_: types::DnsRecordType("TLSA".to_owned()),
        port: Some(443),
        protocol: Some("_tcp".to_owned()),
        ..srv_apex
    };
    assert!(listed_name_matches(&tlsa_apex, "_443._tcp"));
    assert!(!listed_name_matches(&tlsa_apex, "_443._tcp.@"));
}

/// When one record is listed under exactly the name given, only that name's
/// records are selected: `chat` and a record literally named `_sip._tcp.chat`
/// are different owners, and neither selector may reach the other.
#[tokio::test]
async fn fetch_records_does_not_cross_owners_when_a_name_is_also_a_composed_name() {
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/v3/domains/zones/example.com/dns-records")
                .query_param("type", "SRV");
            then.status(200).json_body(json!({
                "items": [srv("r1", "chat", "_sip"), srv("r2", "_sip._tcp.chat", "_sip")],
                "links": []
            }));
        })
        .await;
    let client = client_for(&server);
    let ids = |found: Vec<types::DnsRecord>| -> Vec<String> {
        found.into_iter().filter_map(|r| r.record_id).collect()
    };

    let literal = fetch_records(
        &client,
        "example.com",
        Some("SRV"),
        Some("_sip._tcp.chat"),
        false,
    )
    .await
    .expect("list");
    assert_eq!(ids(literal), ["r2"], "the exact name wins");
    let short = fetch_records(&client, "example.com", Some("SRV"), Some("chat"), false)
        .await
        .expect("list");
    assert_eq!(ids(short), ["r1"]);
}

/// A TLSA name is composed the same way, so it is matched here too.
#[tokio::test]
async fn fetch_records_matches_a_tlsa_name_client_side() {
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/v3/domains/zones/example.com/dns-records")
                .query_param("type", "TLSA")
                .query_param_missing("name");
            then.status(200).json_body(json!({
                "items": [
                    { "recordId": "t1", "type": "TLSA", "name": "www", "ttl": 3600, "port": 443,
                      "protocol": "_tcp", "usage": 3, "selector": 1, "matchingType": 1,
                      "certificateData": "abcd" },
                    { "recordId": "t2", "type": "TLSA", "name": "mail", "ttl": 3600, "port": 25,
                      "protocol": "_tcp", "usage": 3, "selector": 1, "matchingType": 1,
                      "certificateData": "abcd" }
                ],
                "links": []
            }));
        })
        .await;

    let found = fetch_records(
        &client_for(&server),
        "example.com",
        Some("TLSA"),
        Some("www"),
        false,
    )
    .await
    .expect("list");
    let ids: Vec<_> = found
        .iter()
        .filter_map(|r| r.record_id.as_deref())
        .collect();
    assert_eq!(ids, ["t1"]);
}

/// An SRV list with no name has nothing to match, and every other type
/// still filters on the server.
#[tokio::test]
async fn fetch_records_keeps_server_side_filtering_everywhere_else() {
    let server = MockServer::start_async().await;
    let by_name = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/v3/domains/zones/example.com/dns-records")
                .query_param("type", "A")
                .query_param("name", "www");
            then.status(200).json_body(json!({
                "items": [{ "recordId": "a1", "type": "A", "name": "www", "data": "1.2.3.4", "ttl": 3600 }],
                "links": []
            }));
        })
        .await;
    let found = fetch_records(
        &client_for(&server),
        "example.com",
        Some("A"),
        Some("www"),
        false,
    )
    .await
    .expect("list");
    by_name.assert_async().await;
    assert_eq!(found.len(), 1);

    let all_srv = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/v3/domains/zones/example.com/dns-records")
                .query_param("type", "SRV");
            then.status(200)
                .json_body(json!({ "items": [srv("r1", "chat", "_xmpp")], "links": [] }));
        })
        .await;
    let found = fetch_records(
        &client_for(&server),
        "example.com",
        Some("SRV"),
        None,
        false,
    )
    .await
    .expect("list");
    all_srv.assert_async().await;
    assert_eq!(found.len(), 1, "no name given, nothing to filter out");
}

#[test]
fn verified_result_shows_the_requested_form_of_zone_and_name() {
    let payload = || {
        serde_json::json!({
            "domain": "xn--mnchen-3ya.de",
            "name": "xn--bcher-kva",
            "type": "A",
        })
    };
    let unicode = verified_result(
        DomainForm::Unicode,
        payload(),
        "xn--mnchen-3ya.de",
        "A",
        "xn--bcher-kva",
    );
    assert_eq!(unicode.data["domain"], "münchen.de");
    assert_eq!(unicode.data["name"], "bücher");
    let params =
        serde_json::to_string(&unicode.metadata.next_actions).expect("next actions serialize");
    assert!(
        params.contains("münchen.de") && params.contains("bücher"),
        "{params}"
    );

    let ascii = verified_result(
        DomainForm::Punycode,
        payload(),
        "xn--mnchen-3ya.de",
        "A",
        "xn--bcher-kva",
    );
    assert_eq!(ascii.data["domain"], "xn--mnchen-3ya.de");
    assert_eq!(ascii.data["name"], "xn--bcher-kva");
}

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

#[test]
fn parse_write_type_arg_accepts_writable_incl_caa_alias_rejects_ns_soa() {
    assert_eq!(parse_write_type_arg("aaaa").expect("valid"), "AAAA");
    assert_eq!(parse_write_type_arg("caa").expect("valid"), "CAA");
    assert_eq!(parse_write_type_arg("Alias").expect("valid"), "ALIAS");
    assert_eq!(parse_write_type_arg("https").expect("valid"), "HTTPS");
    assert_eq!(parse_write_type_arg("svcb").expect("valid"), "SVCB");
    assert_eq!(parse_write_type_arg("tlsa").expect("valid"), "TLSA");
    // NS/SOA are registry-managed / read-only → rejected with a clear reason.
    for ty in ["NS", "soa"] {
        let err = parse_write_type_arg(ty).expect_err("read-only");
        assert!(err.contains("managed by GoDaddy"), "got: {err}");
    }
    let err = parse_write_type_arg("bogus").expect_err("should reject");
    assert!(err.contains("invalid record type"), "got: {err}");
}

#[test]
fn parse_list_type_arg_also_accepts_ns_soa() {
    assert_eq!(parse_list_type_arg("caa").expect("valid"), "CAA");
    assert_eq!(parse_list_type_arg("ns").expect("valid"), "NS");
    assert_eq!(parse_list_type_arg("SOA").expect("valid"), "SOA");
    assert!(parse_list_type_arg("bogus").is_err());
}

#[test]
fn v3_records_builds_one_per_data_value_with_default_ttl() {
    let recs = v3_records(
        "www",
        "A",
        &["1.2.3.4".to_string(), "5.6.7.8".to_string()],
        &opts(),
    );
    assert_eq!(recs.len(), 2);
    assert_eq!(recs[0].name, "www");
    assert_eq!(recs[0].type_.as_str(), "A");
    assert_eq!(recs[0].data.as_deref(), Some("1.2.3.4"));
    // v3 requires a ttl; an omitted --ttl falls back to the default.
    assert_eq!(recs[0].ttl, DEFAULT_TTL);
    assert_eq!(recs[1].data.as_deref(), Some("5.6.7.8"));
}

#[test]
fn v3_record_carries_srv_and_caa_fields() {
    let mut srv_opts = opts();
    srv_opts.ttl = Some(600);
    srv_opts.priority = Some(10);
    srv_opts.port = Some(443);
    srv_opts.weight = Some(5);
    let srv = v3_record("_sip", "SRV", "sip.example.com", &srv_opts);
    assert_eq!(srv.ttl, 600);
    assert_eq!(srv.priority, Some(10));
    assert_eq!(srv.port, Some(443));
    assert_eq!(srv.weight, Some(5));

    let mut caa_opts = opts();
    caa_opts.flag = Some(128);
    caa_opts.tag = Some("issue".to_string());
    let caa = v3_record("@", "CAA", "letsencrypt.org", &caa_opts);
    assert_eq!(caa.flag, Some(128));
    assert_eq!(caa.tag.as_deref(), Some("issue"));
}

#[test]
fn caa_fields_are_required_for_caa_and_rejected_otherwise() {
    // CAA without --tag → rejected.
    let mut caa_no_tag = opts();
    caa_no_tag.flag = Some(0);
    let err = validate_caa_fields("CAA", &caa_no_tag).expect_err("CAA needs a tag");
    assert!(err.contains("--tag"), "got: {err}");
    // CAA with --tag → ok.
    let mut caa = opts();
    caa.tag = Some("issue".to_string());
    assert!(validate_caa_fields("CAA", &caa).is_ok());
    // --flag/--tag on a non-CAA type → rejected.
    let mut a = opts();
    a.tag = Some("issue".to_string());
    let err = validate_caa_fields("A", &a).expect_err("flag/tag are CAA-only");
    assert!(err.contains("only valid for CAA"), "got: {err}");
    // A non-CAA type with no CAA fields → ok.
    assert!(validate_caa_fields("A", &opts()).is_ok());
}

#[test]
fn v3_record_carries_https_svcb_and_tlsa_fields() {
    let mut https_opts = opts();
    https_opts.priority = Some(1);
    https_opts.parameters = Some("alpn=h2,h3".to_string());
    let https = v3_record("@", "HTTPS", ".", &https_opts);
    assert_eq!(https.priority, Some(1));
    assert_eq!(https.parameters.as_deref(), Some("alpn=h2,h3"));
    assert_eq!(https.data.as_deref(), Some("."));
    assert_eq!(https.certificate_data, None);

    let mut tlsa_opts = opts();
    tlsa_opts.usage = Some(3);
    tlsa_opts.selector = Some(1);
    tlsa_opts.matching_type = Some(1);
    tlsa_opts.protocol = Some("_tcp".to_string());
    tlsa_opts.port = Some(443);
    let cert = "d2abde240d7cd3ee6b4b28c54df034b97983a1d16e8a410e4561cb106618e971";
    let tlsa = v3_record("www", "TLSA", cert, &tlsa_opts);
    // TLSA doesn't use `data` — the value moves to `certificateData`.
    assert_eq!(tlsa.data, None);
    assert_eq!(tlsa.certificate_data.as_deref(), Some(cert));
    assert_eq!(tlsa.usage.map(|u| u.0), Some(3));
    assert_eq!(tlsa.selector.map(|s| s.0), Some(1));
    assert_eq!(tlsa.matching_type.map(|m| m.0), Some(1));
    assert_eq!(tlsa.protocol.as_deref(), Some("_tcp"));
    assert_eq!(tlsa.port, Some(443));
}

#[test]
fn tlsa_fields_are_required_for_tlsa_and_rejected_otherwise() {
    // --usage/--selector/--matching-type on a non-TLSA type → rejected.
    let mut a = opts();
    a.usage = Some(3);
    let err = validate_tlsa_fields("A", &a).expect_err("TLSA fields are TLSA-only");
    assert!(err.contains("only valid for TLSA"), "got: {err}");
    // TLSA with the fields set → ok.
    let mut tlsa = opts();
    tlsa.usage = Some(3);
    tlsa.selector = Some(1);
    tlsa.matching_type = Some(1);
    assert!(validate_tlsa_fields("TLSA", &tlsa).is_ok());
    // A non-TLSA type with no TLSA fields → ok.
    assert!(validate_tlsa_fields("A", &opts()).is_ok());
}

#[test]
fn svcb_parameters_are_rejected_for_other_types() {
    let mut a = opts();
    a.parameters = Some("alpn=h2".to_string());
    let err = validate_svcb_fields("A", &a).expect_err("parameters are HTTPS/SVCB-only");
    assert!(err.contains("only valid for HTTPS/SVCB"), "got: {err}");
    assert!(validate_svcb_fields("HTTPS", &a).is_ok());
    assert!(validate_svcb_fields("SVCB", &a).is_ok());
    assert!(validate_svcb_fields("A", &opts()).is_ok());
}

#[test]
fn record_value_prefers_certificate_data_for_tlsa_even_if_data_is_also_set() {
    // A record we'd never build ourselves (v3_record always nulls one of
    // the two), but defends against the API ever returning both fields
    // populated for a TLSA record — the certificate data must win, not
    // whichever field happens to be checked first.
    let mut tlsa = v3_record(
        "www",
        "TLSA",
        "d2abde240d7cd3ee6b4b28c54df034b97983a1d16e8a410e4561cb106618e971",
        &opts(),
    );
    tlsa.data = Some("stale-or-unrelated".to_string());
    assert_eq!(
        record_value(&tlsa),
        Some("d2abde240d7cd3ee6b4b28c54df034b97983a1d16e8a410e4561cb106618e971")
    );

    // Non-TLSA types are unaffected — `data` still wins.
    let a = v3_record("www", "A", "1.2.3.4", &opts());
    assert_eq!(record_value(&a), Some("1.2.3.4"));
}

#[test]
fn merged_tlsa_data_reconstructs_the_rfc6698_presentation_format() {
    let mut tlsa_opts = opts();
    tlsa_opts.usage = Some(3);
    tlsa_opts.selector = Some(1);
    tlsa_opts.matching_type = Some(1);
    let cert = "d2abde240d7cd3ee6b4b28c54df034b97983a1d16e8a410e4561cb106618e971";
    let tlsa = v3_record("www", "TLSA", cert, &tlsa_opts);
    assert_eq!(merged_tlsa_data(&tlsa), format!("3 1 1 {cert}"));
}

/// The API's schema guarantees usage/selector/matchingType/certificateData
/// together for a real TLSA record, but if one is ever absent, this must
/// not fabricate RDATA by substituting 0 (a real, meaningful value).
#[test]
fn merged_tlsa_data_falls_back_to_certificate_data_when_a_numeric_field_is_missing() {
    let cert = "d2abde240d7cd3ee6b4b28c54df034b97983a1d16e8a410e4561cb106618e971";
    let mut tlsa = v3_record("www", "TLSA", cert, &opts());
    // opts() has no usage/selector/matching_type set, so v3_record leaves
    // them None — exactly the "missing field" case.
    assert_eq!(merged_tlsa_data(&tlsa), cert);

    tlsa.certificate_data = None;
    assert_eq!(merged_tlsa_data(&tlsa), "");
}

#[test]
fn same_content_compares_every_field_except_ttl_and_record_id() {
    let cert = "d2abde240d7cd3ee6b4b28c54df034b97983a1d16e8a410e4561cb106618e971";
    let mut tlsa_opts = opts();
    tlsa_opts.usage = Some(3);
    tlsa_opts.selector = Some(1);
    tlsa_opts.matching_type = Some(1);
    tlsa_opts.ttl = Some(600);
    let mut existing = v3_record("www", "TLSA", cert, &tlsa_opts);
    existing.record_id = Some("r1".to_string());

    // Identical content, different ttl/recordId → still the same record.
    let desired = v3_record("www", "TLSA", cert, &tlsa_opts);
    assert!(same_content(&existing, &desired));

    // Same cert data, different usage → not the same record.
    let mut different_usage = tlsa_opts;
    different_usage.usage = Some(1);
    let desired = v3_record("www", "TLSA", cert, &different_usage);
    assert!(!same_content(&existing, &desired));

    // CAA: same domain value, different tag → not the same record.
    let mut caa_opts = opts();
    caa_opts.tag = Some("issue".to_string());
    let existing_caa = v3_record("@", "CAA", "letsencrypt.org", &caa_opts);
    let mut caa_opts_wild = opts();
    caa_opts_wild.tag = Some("issuewild".to_string());
    let desired_caa = v3_record("@", "CAA", "letsencrypt.org", &caa_opts_wild);
    assert!(!same_content(&existing_caa, &desired_caa));
}
