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

#[derive(Debug, clap::Parser)]
pub(super) struct WriteCli {
    #[command(flatten)]
    pub(super) args: RecordWriteArgs,
}

fn parse_ttl(ttl: &str) -> Result<Option<i64>, clap::Error> {
    use clap::Parser;
    WriteCli::try_parse_from([
        "t",
        "example.com",
        "--type",
        "A",
        "--name",
        "www",
        "--data",
        "1.2.3.4",
        "--ttl",
        ttl,
    ])
    .map(|cli| cli.args.ttl)
}

#[test]
fn ttl_is_limited_to_the_range_the_api_accepts() {
    assert_eq!(parse_ttl("600").expect("lower bound"), Some(600));
    assert_eq!(parse_ttl("86400").expect("upper bound"), Some(86_400));
    for bad in ["599", "1", "0", "86401"] {
        let err = parse_ttl(bad).expect_err("out of range");
        let text = err.to_string();
        assert!(
            text.contains("600") && text.contains("86400"),
            "{bad}: {text}"
        );
    }
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

pub(super) fn opts() -> RecordOptions {
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
        &[opts(), opts()],
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

/// Every flag the API requires for a type, as `(type, flags, a complete set of
/// argv for it)`. The API answers a missing one with a 400 after login; these
/// must be caught when the arguments are parsed.
const REQUIRED_FLAGS_BY_TYPE: &[(&str, &[&str], &[&str])] = &[
    ("MX", &["--priority"], &["--priority", "10"]),
    (
        "SRV",
        &[
            "--priority",
            "--weight",
            "--port",
            "--service",
            "--protocol",
        ],
        &[
            "--priority",
            "1",
            "--weight",
            "1",
            "--port",
            "80",
            "--service",
            "_sip",
            "--protocol",
            "_tcp",
        ],
    ),
    ("HTTPS", &["--priority"], &["--priority", "1"]),
    ("SVCB", &["--priority"], &["--priority", "1"]),
    (
        "TLSA",
        &[
            "--usage",
            "--selector",
            "--matching-type",
            "--port",
            "--protocol",
        ],
        &[
            "--usage",
            "3",
            "--selector",
            "1",
            "--matching-type",
            "1",
            "--port",
            "443",
            "--protocol",
            "_tcp",
        ],
    ),
    ("CAA", &["--tag"], &["--tag", "issue"]),
];

fn parse_type(record_type: &str, extra: &[&str]) -> Result<WriteCli, clap::Error> {
    use clap::Parser;
    let mut argv = vec![
        "t",
        "example.com",
        "--type",
        record_type,
        "--name",
        "n",
        "--data",
        "x",
    ];
    argv.extend_from_slice(extra);
    WriteCli::try_parse_from(argv)
}

/// Drop `flag` and its value from a complete argv.
fn without(flag: &str, argv: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    let mut skip = false;
    for (i, arg) in argv.iter().enumerate() {
        if skip {
            skip = false;
        } else if *arg == flag {
            skip = i + 1 < argv.len();
        } else {
            out.push((*arg).to_owned());
        }
    }
    out
}

#[test]
fn each_type_accepts_its_complete_set_of_flags() {
    for (record_type, _, complete) in REQUIRED_FLAGS_BY_TYPE {
        parse_type(record_type, complete).expect(record_type);
    }
}

#[test]
fn each_required_flag_is_reported_by_name_when_missing() {
    for (record_type, required, complete) in REQUIRED_FLAGS_BY_TYPE {
        for flag in *required {
            let argv = without(flag, complete);
            let refs: Vec<&str> = argv.iter().map(String::as_str).collect();
            let err = parse_type(record_type, &refs)
                .expect_err(&format!("{record_type} without {flag} should not parse"));
            let text = err.to_string();
            assert!(text.contains(flag), "{record_type} without {flag}: {text}");
        }
    }
}

#[test]
fn the_lower_case_type_requires_the_same_flags() {
    // `ignore_case` on --type is what makes the conditions match `--type srv`.
    let err = parse_type("srv", &["--priority", "1"]).expect_err("srv needs more");
    assert!(err.to_string().contains("--weight"), "{err}");
    let err = parse_type(
        "tlsa",
        &["--usage", "3", "--selector", "1", "--matching-type", "1"],
    )
    .expect_err("tlsa needs port and protocol");
    assert!(err.to_string().contains("--port"), "{err}");
}

#[test]
fn types_that_need_none_of_them_still_parse_with_none() {
    for record_type in ["A", "AAAA", "CNAME", "TXT"] {
        parse_type(record_type, &[]).expect(record_type);
    }
}

#[test]
fn the_documented_requirements_match_what_clap_enforces() {
    // spec_cmd reads CONDITIONALLY_REQUIRED_FLAGS by hand because clap cannot
    // expose required_if_eq; a flag listed there must really be required for the
    // types it names, and every flag clap requires must be listed.
    let listed: std::collections::BTreeSet<&str> = CONDITIONALLY_REQUIRED_FLAGS
        .iter()
        .map(|(flag, _)| *flag)
        .collect();
    let enforced: std::collections::BTreeSet<&str> = REQUIRED_FLAGS_BY_TYPE
        .iter()
        .flat_map(|(_, required, _)| required.iter().map(|f| f.trim_start_matches("--")))
        .collect();
    assert_eq!(listed, enforced);
}

#[test]
fn service_and_protocol_gain_the_underscore_the_api_requires() {
    // With or without it, the same label comes out.
    for (given, want) in [
        ("sip", "_sip"),
        ("_sip", "_sip"),
        ("tcp", "_tcp"),
        ("_tcp", "_tcp"),
        ("x509", "_x509"),
        ("a-b", "_a-b"),
        ("a_b", "_a_b"),
        ("SIP", "_SIP"),
        ("_SIP", "_SIP"),
        // A deliberate second underscore is the customer's own and is kept.
        ("__sip", "__sip"),
        ("sip_", "_sip_"),
    ] {
        assert_eq!(
            parse_underscore_label(given).as_deref(),
            Ok(want),
            "{given}"
        );
    }
    // 63 characters in all is the longest label the API accepts, underscore
    // included: 62 given characters become 63, and 63 given become 64.
    let longest = "a".repeat(62);
    assert_eq!(
        parse_underscore_label(&longest).as_deref(),
        Ok(format!("_{longest}").as_str())
    );
    assert!(parse_underscore_label(&format!("{longest}a")).is_err());
    assert!(parse_underscore_label(&format!("_{longest}a")).is_err());

    for bad in ["", "_", " sip", "_ sip", "sip.x", "_sip.x", "é", "_é"] {
        let err = parse_underscore_label(bad).expect_err(bad);
        assert!(err.contains("sip") && err.contains("62"), "{bad}: {err}");
    }
}

#[test]
fn service_and_protocol_are_normalized_when_the_arguments_are_parsed() {
    use clap::Parser;
    // The other flags SRV requires are always given, so only the value under
    // test can make the parse fail.
    let parse = |service: &str, protocol: &str| {
        WriteCli::try_parse_from([
            "t",
            "example.com",
            "--type",
            "SRV",
            "--name",
            "chat",
            "--data",
            "t.example.com.",
            "--priority",
            "1",
            "--weight",
            "1",
            "--port",
            "80",
            "--service",
            service,
            "--protocol",
            protocol,
        ])
    };
    // Plain and underscored spellings are the same request.
    for (service, protocol) in [
        ("sip", "tcp"),
        ("_sip", "_tcp"),
        ("sip", "_tcp"),
        ("_sip", "tcp"),
    ] {
        let ok = parse(service, protocol).expect("valid");
        assert_eq!(
            ok.args.service.first().map(String::as_str),
            Some("_sip"),
            "{service} {protocol}"
        );
        assert_eq!(
            ok.args.protocol.first().map(String::as_str),
            Some("_tcp"),
            "{service} {protocol}"
        );
    }

    // A value that cannot be a label is still caught before any request.
    for (flag, service, protocol) in [("--service", "si p", "tcp"), ("--protocol", "sip", "t.cp")] {
        let err = parse(service, protocol).expect_err("not a label");
        let text = err.to_string();
        assert!(
            text.contains(flag) && text.contains("letters"),
            "{flag}: {text}"
        );
    }
}

#[test]
fn the_full_dns_name_is_rejected_for_srv_and_tlsa_and_the_message_says_what_to_use() {
    let err = validate_record_name("SRV", "_sip._tcp.chat").expect_err("full name");
    assert!(
        err.contains("--name chat") && err.contains("--service/--protocol"),
        "{err}"
    );
    assert!(err.contains("_sip._tcp"), "{err}");

    // At the apex the record's own name is `@`.
    let err = validate_record_name("SRV", "_sip._tcp").expect_err("apex full name");
    assert!(err.contains("--name @"), "{err}");

    // A name with dots below the record keeps them.
    let err = validate_record_name("SRV", "_sip._tcp.a.b").expect_err("nested");
    assert!(err.contains("--name a.b"), "{err}");

    let err = validate_record_name("TLSA", "_443._tcp.www").expect_err("tlsa full name");
    assert!(
        err.contains("--name www") && err.contains("--port/--protocol"),
        "{err}"
    );
}

#[test]
fn the_records_own_name_and_every_other_type_are_accepted() {
    for name in ["chat", "@", "a.b", "_underscored", "_one.two"] {
        assert!(validate_record_name("SRV", name).is_ok(), "{name}");
        assert!(validate_record_name("TLSA", name).is_ok(), "{name}");
    }
    // Only SRV and TLSA have a composed name; an underscore name is ordinary elsewhere.
    for ty in ["A", "TXT", "CNAME", "MX", "CAA"] {
        assert!(
            validate_record_name(ty, "_dmarc._tcp.example").is_ok(),
            "{ty}"
        );
    }
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
fn a_caa_record_without_a_flag_is_non_critical() {
    let mut caa = opts();
    caa.tag = Some("issue".to_owned());
    assert_eq!(v3_record("@", "CAA", "letsencrypt.org", &caa).flag, Some(0));
    // An explicit flag wins, and other types never get one.
    caa.flag = Some(128);
    assert_eq!(
        v3_record("@", "CAA", "letsencrypt.org", &caa).flag,
        Some(128)
    );
    assert_eq!(v3_record("@", "A", "1.2.3.4", &opts()).flag, None);

    // So an omitted --flag matches a stored flag-0 record.
    let mut stored = v3_record("@", "CAA", "letsencrypt.org", &{
        let mut o = opts();
        o.flag = Some(0);
        o.tag = Some("issue".to_owned());
        o
    });
    stored.record_id = Some("r1".to_owned());
    let mut no_flag = opts();
    no_flag.tag = Some("issue".to_owned());
    assert!(same_content(
        &stored,
        &v3_record("@", "CAA", "letsencrypt.org", &no_flag)
    ));
}

#[test]
fn same_content_follows_how_the_api_stores_each_field() {
    let same = |ty: &str, a: &str, b: &str| {
        same_content(
            &v3_record("w", ty, a, &opts()),
            &v3_record("w", ty, b, &opts()),
        )
    };
    // Hostnames: case and a trailing dot are not differences.
    assert!(same("CNAME", "Example.COM", "example.com."));
    assert!(same("ALIAS", "Example.COM", "example.com."));
    assert!(same("ALIAS", "Example.COM", "example.com."));
    assert!(same("MX", "Mail.Example.com", "mail.example.com."));
    // CAA data and TLSA hex are case-insensitive.
    assert!(same("CAA", "Letsencrypt.org", "letsencrypt.org"));
    // Text the API keeps as written is compared exactly.
    assert!(!same("TXT", "Hello", "hello"));
    assert!(!same("AAAA", "2001:DB8::1", "2001:db8::1"));
    let mut a = opts();
    a.parameters = Some("ech=AbCd==".to_owned());
    let mut b = opts();
    b.parameters = Some("ech=abcd==".to_owned());
    assert!(!same_content(
        &v3_record("w", "HTTPS", ".", &a),
        &v3_record("w", "HTTPS", ".", &b)
    ));
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

#[test]
fn the_detail_columns_cover_exactly_the_detail_fields_and_all_hide_when_empty() {
    let columns = detail_columns();
    let fields: Vec<&str> = columns.iter().map(|c| c.field.as_str()).collect();
    assert_eq!(fields, DETAIL_FIELDS);
    assert!(
        columns.iter().all(|c| c.hide_when_empty),
        "every detail column is sparse"
    );
}

#[test]
fn detail_values_carry_only_the_fields_a_record_has() {
    let a = v3_record("www", "A", "1.2.3.4", &opts());
    assert!(
        detail_values(&a).is_empty(),
        "an A record has no detail fields"
    );

    let mut srv_opts = opts();
    srv_opts.priority = Some(10);
    srv_opts.weight = Some(5);
    srv_opts.port = Some(5222);
    srv_opts.service = Some("_xmpp".to_owned());
    srv_opts.protocol = Some("_tcp".to_owned());
    let values = detail_values(&v3_record("chat", "SRV", "t.example.com.", &srv_opts));
    assert_eq!(values["service"], "_xmpp");
    assert_eq!(values["port"], 5222);
    assert_eq!(values["priority"], 10);
    assert!(!values.contains_key("tag") && !values.contains_key("data"));
}

/// The API keeps an HTTPS/SVCB root target as `.`; trimming dots to compare must
/// not turn it into an empty value in a preview.
#[test]
fn as_stored_keeps_the_root_target_of_an_https_record() {
    let https_opts = RecordOptions {
        priority: Some(0),
        ..opts()
    };
    let record = v3_record("@", "HTTPS", ".", &https_opts);
    assert_eq!(as_stored(&record).data.as_deref(), Some("."));

    let host = v3_record("@", "CNAME", "Target.Example.COM", &opts());
    assert_eq!(
        as_stored(&host).data.as_deref(),
        Some("target.example.com.")
    );

    let empty = types::DnsRecord {
        data: None,
        ..v3_record("@", "CNAME", "x", &opts())
    };
    assert_eq!(as_stored(&empty).data, None);
}
