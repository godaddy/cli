use super::*;
use crate::dns::records::{RecordOptions, same_content, v3_record};

fn opts() -> RecordOptions {
    blank_opts(3600)
}

/// A record as the API lists it.
fn listed(ty: &str, name: &str, value: &str, opts: &RecordOptions) -> types::DnsRecord {
    types::DnsRecord {
        record_id: Some(format!("id-{ty}-{name}")),
        ..v3_record(name, ty, value, opts)
    }
}

fn imported(text: &str) -> Imported {
    from_zone_text(text, "example.com").expect("imports")
}

fn one(text: &str) -> Desired {
    let mut imported = imported(text);
    assert_eq!(imported.desired.len(), 1, "{:?}", imported.desired);
    imported.desired.remove(0)
}

// ------------------------------------------------------------------ zone in

#[test]
fn zone_records_become_the_pieces_dns_add_builds_from() {
    let d = one("www 600 IN A 192.0.2.1\n");
    assert_eq!(
        (d.record_type.as_str(), d.name.as_str(), d.value.as_str()),
        ("A", "www", "192.0.2.1")
    );
    assert_eq!(d.opts.ttl, Some(600));
    assert_eq!(d.at, "line 1");
}

#[test]
fn a_zone_without_origin_or_ttl_lines_uses_the_domain_and_the_default_ttl() {
    let d = one("www A 192.0.2.1\n");
    assert_eq!(d.opts.ttl, Some(3600));
    let apex = one("@ MX 10 mail\n");
    assert_eq!(apex.name, "@");
    assert_eq!(apex.value, "mail.example.com.");
    assert_eq!(apex.opts.priority, Some(10));
}

#[test]
fn names_are_made_relative_to_the_domain_and_lower_cased() {
    let d = one("WWW.Example.COM. 600 A 192.0.2.1\n");
    assert_eq!(d.name, "www");
    let apex = one("example.com. 600 A 192.0.2.1\n");
    assert_eq!(apex.name, "@");
}

#[test]
fn a_name_outside_the_domain_is_rejected_with_a_hint() {
    let errors = from_zone_text("www.other.org. 600 A 192.0.2.1\n", "example.com")
        .expect_err("outside the domain");
    assert_eq!(errors.len(), 1);
    assert!(errors[0].starts_with("line 1: "), "{errors:?}");
    assert!(errors[0].contains("$ORIGIN example.com."), "{errors:?}");
}

#[test]
fn a_unicode_owner_name_is_read_as_unicode_so_it_can_become_punycode() {
    let d = one("café 600 A 192.0.2.1\n");
    assert_eq!(
        d.name, "café",
        "not the escaped bytes of its UTF-8 encoding"
    );

    // A name with an ASCII-only form is unchanged by this.
    assert_eq!(one("www 600 A 192.0.2.1\n").name, "www");
    assert_eq!(one("@ 600 A 192.0.2.1\n").name, "@");
}

#[test]
fn srv_owner_splits_into_service_protocol_and_name() {
    let d = one("_sip._tcp.chat 600 SRV 10 60 5060 sip.example.net.\n");
    assert_eq!((d.record_type.as_str(), d.name.as_str()), ("SRV", "chat"));
    assert_eq!(d.opts.service.as_deref(), Some("_sip"));
    assert_eq!(d.opts.protocol.as_deref(), Some("_tcp"));
    assert_eq!(
        (d.opts.priority, d.opts.weight, d.opts.port),
        (Some(10), Some(60), Some(5060))
    );
    assert_eq!(d.value, "sip.example.net.");

    let apex = one("_sip._tcp 600 SRV 10 60 5060 sip.example.net.\n");
    assert_eq!(apex.name, "@");
}

#[test]
fn tlsa_owner_splits_into_port_protocol_and_name_and_keeps_the_hex() {
    let d = one(
        "_443._tcp.www 600 TLSA 3 1 1 D2ABDE240D7CD3EE6B4B28C54DF034B97983A1D16E8A410E4561CB106618E971\n",
    );
    assert_eq!((d.record_type.as_str(), d.name.as_str()), ("TLSA", "www"));
    assert_eq!(
        (d.opts.port, d.opts.protocol.as_deref()),
        (Some(443), Some("_tcp"))
    );
    assert_eq!(
        (d.opts.usage, d.opts.selector, d.opts.matching_type),
        (Some(3), Some(1), Some(1))
    );
    assert_eq!(
        d.value,
        "d2abde240d7cd3ee6b4b28c54df034b97983a1d16e8a410e4561cb106618e971"
    );
}

#[test]
fn srv_and_tlsa_owners_without_the_prefix_are_rejected() {
    for text in [
        "chat 600 SRV 10 60 5060 sip.example.net.\n",
        "www 600 TLSA 3 1 1 d2ab\n",
        "_x._tcp.www 600 TLSA 3 1 1 d2ab\n",
    ] {
        let errors = from_zone_text(text, "example.com").expect_err(text);
        assert!(errors[0].starts_with("line 1: "), "{errors:?}");
    }
}

#[test]
fn txt_chunks_are_joined_and_caa_and_https_carry_their_fields() {
    let long = "x".repeat(300);
    let d = one(&format!(
        "t 600 TXT \"{}\" \"{}\"\n",
        &long[..255],
        &long[255..]
    ));
    assert_eq!(d.value, long, "chunks are one value again");

    let caa = one("@ 600 CAA 128 issue \"letsencrypt.org\"\n");
    assert_eq!(caa.opts.flag, Some(128));
    assert_eq!(caa.opts.tag.as_deref(), Some("issue"));
    assert_eq!(caa.value, "letsencrypt.org");

    let https = one("@ 600 HTTPS 1 . alpn=h2,h3\n");
    assert_eq!(https.value, ".");
    assert_eq!(https.opts.priority, Some(1));
    assert_eq!(https.opts.parameters.as_deref(), Some("alpn=h2,h3"));
}

#[test]
fn a_record_type_the_api_does_not_have_is_reported_with_its_line_not_imported() {
    // ALIAS is in the API's spec but refused by it in test and prod, so a zone file
    // that has one fails here, with the line, instead of as a 400 per record.
    let errors = from_zone_text(
        "www A 192.0.2.1\n@ 600 ALIAS lb.example.net.\n",
        "example.com",
    )
    .expect_err("unknown type");
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].contains("line 2"), "{errors:?}");
    assert!(errors[0].contains("ALIAS"), "{errors:?}");
}

#[test]
fn ns_and_soa_are_skipped_not_imported() {
    let imported = imported(
        "@ SOA ns1 admin 1 7200 900 1209600 300\n\
         @ NS ns1.example.com.\n\
         sub NS ns.other.net.\n\
         www A 192.0.2.1\n",
    );
    assert_eq!(imported.desired.len(), 1);
    let skipped: Vec<_> = imported
        .skipped
        .iter()
        .map(|s| (s.record_type.as_str(), s.name.as_str(), s.at.as_str()))
        .collect();
    assert_eq!(
        skipped,
        [
            ("SOA", "@", "line 1"),
            ("NS", "@", "line 2"),
            ("NS", "sub", "line 3")
        ]
    );
}

#[test]
fn every_bad_record_is_reported_with_its_line_not_just_the_first() {
    // Records the parser accepts but that cannot be imported, on separate lines.
    let errors = from_zone_text(
        "a.other.org. 600 A 192.0.2.1\nok A 192.0.2.2\nb.other.org. 600 A 192.0.2.3\n",
        "example.com",
    )
    .expect_err("two outside");
    assert_eq!(errors.len(), 2);
    assert!(
        errors[0].starts_with("line 1:") && errors[1].starts_with("line 3:"),
        "{errors:?}"
    );
}

#[test]
fn a_syntax_error_in_the_file_is_reported_with_its_position() {
    let errors = from_zone_text("www A not-an-ip\n", "example.com").expect_err("bad");
    assert_eq!(errors.len(), 1);
    assert!(errors[0].contains("line 1"), "{errors:?}");
}

// ------------------------------------------------------------------- JSON in

#[test]
fn json_input_may_be_the_list_or_the_whole_envelope() {
    let list = r#"[{"type":"A","name":"www","data":"192.0.2.1","ttl":600}]"#;
    let envelope = format!(r#"{{"data":{list},"metadata":{{}}}}"#);
    for text in [list.to_owned(), envelope] {
        let imported = from_json_text(&text).expect("reads");
        assert_eq!(imported.desired.len(), 1, "{text}");
        assert_eq!(imported.desired[0].at, "record 1");
        assert_eq!(imported.desired[0].opts.ttl, Some(600));
    }
}

#[test]
fn json_input_that_is_not_a_list_of_records_says_so() {
    for text in ["{}", "3", "\"x\"", "{\"data\": {}}", "not json"] {
        let errors = from_json_text(text).expect_err(text);
        assert_eq!(errors.len(), 1, "{text}");
    }
}

#[test]
fn a_json_record_with_no_data_is_refused_but_an_explicit_empty_value_is_not() {
    let errors = from_json_text(r#"[{"type":"TXT","name":"@","ttl":600}]"#).expect_err("no data");
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].starts_with("record 1: "), "{errors:?}");
    assert!(errors[0].contains("no `data`"), "{errors:?}");

    // `data: ""` is a value someone wrote, so it stays distinct from a missing one.
    let imported = from_json_text(r#"[{"type":"TXT","name":"@","ttl":600,"data":""}]"#)
        .expect("an explicit empty TXT value reads");
    assert_eq!(imported.desired[0].value, "");

    // A TLSA record may carry its value as `certificateData` alone.
    let imported = from_json_text(
        r#"[{"type":"TLSA","name":"www","ttl":600,"port":443,"protocol":"_tcp",
             "usage":3,"selector":1,"matchingType":1,"certificateData":"d2abde24"}]"#,
    )
    .expect("reads");
    assert_eq!(imported.desired[0].value, "d2abde24");
}

#[test]
fn json_records_keep_their_fields_and_skip_ns_and_soa() {
    let imported = from_json_text(
        r#"[
          {"type":"MX","name":"@","data":"mail.example.com.","priority":10,"ttl":600},
          {"type":"NS","name":"@","data":"ns1.example.com.","ttl":3600},
          {"type":"soa","name":"@","data":"x","ttl":3600},
          {"type":"CAA","name":"@","data":"letsencrypt.org","flag":0,"tag":"issue","ttl":600}
        ]"#,
    )
    .expect("reads");
    assert_eq!(imported.desired.len(), 2);
    assert_eq!(imported.skipped.len(), 2);
    assert_eq!(imported.desired[0].opts.priority, Some(10));
    assert_eq!(imported.desired[1].opts.tag.as_deref(), Some("issue"));
}

#[test]
fn json_from_dns_list_reads_back_a_tlsa_record_whose_data_is_merged() {
    // `dns list -o json` puts "usage selector matching-type cert" in `data`.
    let imported = from_json_text(
        r#"[{"type":"TLSA","name":"www","port":443,"protocol":"_tcp","ttl":600,
             "data":"3 1 1 d2abde24","certificateData":"d2abde24",
             "usage":3,"selector":1,"matchingType":1}]"#,
    )
    .expect("reads");
    let d = &imported.desired[0];
    assert_eq!(
        d.value, "d2abde24",
        "the certificate data, not the merged text"
    );

    let merged_only = from_json_text(
        r#"[{"type":"TLSA","name":"www","port":443,"protocol":"_tcp","ttl":600,
             "data":"3 1 1 d2abde24"}]"#,
    )
    .expect("reads");
    let d = &merged_only.desired[0];
    assert_eq!(d.value, "d2abde24");
    assert_eq!(
        (d.opts.usage, d.opts.selector, d.opts.matching_type),
        (Some(3), Some(1), Some(1))
    );

    let bad = from_json_text(r#"[{"type":"TLSA","name":"www","ttl":600,"data":"d2abde24"}]"#)
        .expect_err("no usage");
    assert!(bad[0].starts_with("record 1: "), "{bad:?}");
}

#[test]
fn a_json_record_that_does_not_fit_the_api_shape_is_reported_by_position() {
    let errors =
        from_json_text(r#"[{"type":"A","name":"www","ttl":600,"data":"192.0.2.1"},{"name":"x"}]"#)
            .expect_err("missing fields");
    assert_eq!(errors.len(), 1);
    assert!(errors[0].starts_with("record 2: "), "{errors:?}");
}

// ----------------------------------------------------------------- zone out

fn exported(records: &[types::DnsRecord]) -> String {
    export_zone(records, "example.com", &[]).expect("exports")
}

#[test]
fn an_exported_zone_reads_back_to_the_same_records() {
    let mx = RecordOptions {
        priority: Some(10),
        ..opts()
    };
    let srv = RecordOptions {
        priority: Some(10),
        weight: Some(60),
        port: Some(5060),
        service: Some("_sip".into()),
        protocol: Some("_tcp".into()),
        ..opts()
    };
    let tlsa = RecordOptions {
        port: Some(443),
        protocol: Some("_tcp".into()),
        usage: Some(3),
        selector: Some(1),
        matching_type: Some(1),
        ..opts()
    };
    let caa = RecordOptions {
        flag: Some(0),
        tag: Some("issue".into()),
        ..opts()
    };
    let https = RecordOptions {
        priority: Some(1),
        parameters: Some("alpn=h2,h3".into()),
        ..opts()
    };
    let cert = "d2abde240d7cd3ee6b4b28c54df034b97983a1d16e8a410e4561cb106618e971";
    let records = vec![
        listed("A", "www", "192.0.2.1", &opts()),
        listed("AAAA", "www", "2001:db8::1", &opts()),
        listed("CNAME", "blog", "www.example.com.", &opts()),
        listed("MX", "@", "mail.example.com.", &mx),
        listed("TXT", "@", "v=spf1 -all", &opts()),
        listed("TXT", "k._domainkey", &"k".repeat(400), &opts()),
        listed("SRV", "chat", "sip.example.net.", &srv),
        listed("TLSA", "www", cert, &tlsa),
        listed("CAA", "@", "letsencrypt.org", &caa),
        listed("HTTPS", "@", ".", &https),
    ];
    let text = exported(&records);
    let back = from_zone_text(&text, "example.com").expect(&text);
    assert!(back.skipped.is_empty());
    assert_eq!(back.desired.len(), records.len(), "{text}");
    for want in &records {
        let found = back
            .desired
            .iter()
            .any(|d| same_content(&d.record(), want) && d.opts.ttl == Some(want.ttl));
        assert!(
            found,
            "{} {} missing from the round trip:\n{text}",
            want.type_.as_str(),
            want.name
        );
    }
}

#[test]
fn export_writes_the_domains_own_records_first_then_the_rest_by_name() {
    let text = exported(&[
        listed("A", "www", "192.0.2.1", &opts()),
        listed("TXT", "_dmarc", "v=DMARC1", &opts()),
        listed("A", "@", "192.0.2.9", &opts()),
    ]);
    let owners: Vec<&str> = text
        .lines()
        .filter(|l| !l.starts_with('$'))
        .filter_map(|l| l.split_whitespace().next())
        .collect();
    assert_eq!(owners, ["@", "_dmarc", "www"], "{text}");
    assert!(text.starts_with("$ORIGIN example.com.\n"), "{text}");
}

#[test]
fn the_soa_and_the_domains_own_ns_records_are_provider_managed_and_other_ns_records_are_not() {
    let managed = |ty: &str, name: &str| is_provider_managed(&listed(ty, name, "x", &opts()));
    assert!(managed("SOA", "@"));
    assert!(managed("soa", "@"));
    assert!(managed("NS", "@"));
    assert!(managed("ns", "@"));

    // A delegation hands a subdomain to other name servers: the owner's data.
    assert!(!managed("NS", "sub"));
    assert!(!managed("NS", "a.b"));
    // Only NS and SOA are ever provider-managed by type; the marker is by value.
    assert!(!managed("A", "@"));
    assert!(!managed("MX", "@"));
    assert!(!managed("TXT", "@"));
}

#[test]
fn the_parked_marker_is_provider_managed_whatever_its_name_or_case() {
    let managed =
        |ty: &str, name: &str, value: &str| is_provider_managed(&listed(ty, name, value, &opts()));
    assert!(managed("A", "@", "Parked"));
    assert!(managed("A", "@", "parked"));
    assert!(
        managed("a", "www", "PARKED"),
        "a subdomain can be parked too"
    );
    assert!(managed("AAAA", "@", "Parked"));
    assert!(
        managed("A", "@", " Parked "),
        "surrounding spaces do not hide it"
    );

    // Only that word, and only as an address: other values and types are the owner's.
    assert!(!managed("A", "@", "192.0.2.1"));
    assert!(!managed("A", "@", "Parkedish"));
    assert!(!managed("A", "@", "placeholder"));
    assert!(
        !managed("TXT", "@", "Parked"),
        "a TXT value of Parked is just text"
    );
    assert!(!managed("CNAME", "www", "Parked"));
}

#[test]
fn hostnames_from_the_api_are_never_taken_as_relative() {
    // The API may return a hostname without its final dot; it is still that host.
    let text = exported(&[listed("CNAME", "www", "target.example.net", &opts())]);
    assert!(text.contains("CNAME target.example.net."), "{text}");
    assert!(!text.contains("target.example.net.example.com"), "{text}");
}

#[test]
fn export_names_each_record_it_cannot_write() {
    let broken = types::DnsRecord {
        data: Some("not-an-ip".into()),
        ..listed("A", "bad", "x", &opts())
    };
    let no_priority = listed("MX", "mx", "mail.example.com.", &opts());
    let errors = export_zone(&[broken, no_priority], "example.com", &[]).expect_err("two bad");
    assert_eq!(errors.len(), 2, "{errors:?}");
    assert!(errors[0].starts_with("A bad not-an-ip"), "{errors:?}");
    assert!(errors[1].contains("no priority"), "{errors:?}");
}

#[test]
fn nonstandard_records_are_split_off_and_the_rest_keep_their_order() {
    let all = vec![
        listed("A", "www", "192.0.2.1", &opts()),
        listed("DNAME", "@", "lb.example.net.", &opts()),
        listed("MX", "@", "mail.example.com.", &opts()),
        listed("dname", "apex2", "lb2.example.net.", &opts()),
    ];
    let (standard, left_out) = split_nonstandard(&all);
    let types = |records: &[types::DnsRecord]| {
        records
            .iter()
            .map(|r| r.type_.as_str().to_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(types(&standard), ["A", "MX"]);
    assert_eq!(
        types(&left_out),
        ["DNAME", "dname"],
        "matched without regard to case"
    );
}

#[test]
fn a_placeholder_instead_of_an_address_is_nonstandard_and_a_real_address_is_not() {
    let nonstandard = |ty: &str, value: &str| is_nonstandard(&listed(ty, "@", value, &opts()));
    // A word where an address belongs.
    assert!(nonstandard("A", "placeholder"));
    assert!(
        nonstandard("a", "placeholder"),
        "type matched without regard to case"
    );
    assert!(nonstandard("A", ""));
    assert!(
        nonstandard("A", "2001:db8::1"),
        "an IPv6 address is not an A value"
    );
    assert!(nonstandard("AAAA", "placeholder"));
    assert!(
        nonstandard("AAAA", "192.0.2.1"),
        "an IPv4 address is not an AAAA value"
    );
    assert!(nonstandard("DNAME", "lb.example.net."));

    assert!(!nonstandard("A", "192.0.2.1"));
    assert!(!nonstandard("AAAA", "2001:db8::1"));
    assert!(!nonstandard("CNAME", "www.example.com."));
    assert!(
        !nonstandard("TXT", "placeholder"),
        "only addresses are checked for a placeholder"
    );
}

#[test]
fn what_the_split_calls_standard_is_always_writable() {
    // The split and the writer must agree: a record the split keeps cannot make
    // the export fail on its address.
    let all = vec![
        listed("A", "ok", "192.0.2.1", &opts()),
        listed("A", "@", "placeholder", &opts()),
        listed("AAAA", "v6", "2001:db8::1", &opts()),
        listed("AAAA", "bad6", "nope", &opts()),
        listed("DNAME", "apex", "lb.example.net.", &opts()),
    ];
    let (standard, left_out) = split_nonstandard(&all);
    assert_eq!(standard.len(), 2);
    assert_eq!(left_out.len(), 3);
    export_zone(&standard, "example.com", &left_out).expect("everything kept can be written");
}

#[test]
fn the_left_out_comment_shows_the_placeholder_value() {
    let all = vec![
        listed("A", "www", "192.0.2.1", &opts()),
        listed("A", "@", "placeholder", &opts()),
    ];
    let (standard, left_out) = split_nonstandard(&all);
    let text = export_zone(&standard, "example.com", &left_out).expect("exports");
    assert!(text.contains(";   A @ placeholder"), "{text}");
    assert!(
        !text
            .lines()
            .any(|l| !l.starts_with(';') && l.contains("placeholder")),
        "{text}"
    );
}

#[test]
fn a_zone_file_without_nonstandard_records_lists_what_was_left_out_in_comments() {
    let all = vec![
        listed("A", "www", "192.0.2.1", &opts()),
        listed("DNAME", "@", "lb.example.net.", &opts()),
    ];
    let (standard, left_out) = split_nonstandard(&all);
    let text = export_zone(&standard, "example.com", &left_out).expect("exports");

    assert!(
        text.starts_with("; Left out (1 record(s) a standard DNS provider can't use)"),
        "{text}"
    );
    assert!(text.contains(";   DNAME @ lb.example.net."), "{text}");
    // The DNAME record is only in the comment, never a record line.
    assert!(
        !text
            .lines()
            .any(|l| !l.starts_with(';') && l.contains("DNAME")),
        "{text}"
    );
    assert!(text.contains(" IN A 192.0.2.1"), "{text}");

    // And the file loads where DNAME is not accepted: it has no DNAME record.
    let strict = rfc1035_format::parse(&text, &ParseOptions::default().with_default_ttl(600))
        .expect("a strict reader accepts it");
    assert_eq!(strict.records.len(), 1);
}

#[test]
fn a_record_that_is_not_left_out_and_cannot_be_written_stops_the_export_naming_it() {
    // Without --standard-only a record the writer cannot use is an error, so a file
    // never holds a record another provider would reject.
    let errors = export_zone(
        &[
            listed("A", "www", "192.0.2.1", &opts()),
            listed("DNAME", "@", "target.example.net.", &opts()),
        ],
        "example.com",
        &[],
    )
    .expect_err("cannot write a DNAME");
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(
        errors[0].starts_with("DNAME @ target.example.net."),
        "{errors:?}"
    );
}

#[test]
fn a_zone_file_with_only_standard_records_has_no_warning_comment() {
    let text = exported(&[listed("A", "www", "192.0.2.1", &opts())]);
    assert!(!text.contains("Note:"), "{text}");
    assert!(!text.contains("Left out"), "{text}");
}

#[test]
fn nothing_left_out_means_no_comment() {
    let (standard, left_out) = split_nonstandard(&[listed("A", "www", "192.0.2.1", &opts())]);
    assert!(left_out.is_empty());
    let text = export_zone(&standard, "example.com", &left_out).expect("exports");
    assert!(!text.contains("Left out"), "{text}");
}
