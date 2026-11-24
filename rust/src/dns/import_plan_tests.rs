use super::*;
use crate::dns::records::{RecordOptions, v3_record};

fn opts(ttl: i64) -> RecordOptions {
    RecordOptions {
        ttl: Some(ttl),
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

fn want(at: usize, ty: &str, name: &str, value: &str, ttl: i64) -> Desired {
    Desired {
        at: format!("line {at}"),
        record_type: ty.to_owned(),
        name: name.to_owned(),
        value: value.to_owned(),
        opts: opts(ttl),
    }
}

/// A record the zone already has.
fn have(id: &str, ty: &str, name: &str, value: &str, ttl: i64) -> types::DnsRecord {
    types::DnsRecord {
        record_id: Some(id.to_owned()),
        ..v3_record(name, ty, value, &opts(ttl))
    }
}

fn plan(desired: Vec<Desired>, existing: &[types::DnsRecord], replace: bool) -> ImportPlan {
    build_plan("example.com", desired, existing, replace).expect("plans")
}

fn kinds(group: &Group) -> Vec<&'static str> {
    group
        .actions
        .iter()
        .map(|a| match a {
            SetAction::Create { .. } => "create",
            SetAction::Replace { .. } => "replace",
            SetAction::Unchanged { .. } => "unchanged",
            SetAction::Delete { .. } => "delete",
        })
        .collect()
}

fn group<'a>(plan: &'a ImportPlan, ty: &str, owner: &str) -> &'a Group {
    plan.groups
        .iter()
        .find(|g| g.record_type == ty && g.owner == owner)
        .expect("the plan has a group for this type and owner")
}

// -------------------------------------------------------------- validation

#[test]
fn a_valid_record_passes_and_its_type_is_upper_cased() {
    let mut records = vec![want(1, "a", "www", "192.0.2.1", 600)];
    assert!(validate(&mut records).is_empty());
    assert_eq!(records[0].record_type, "A");
}

#[test]
fn ttl_outside_the_range_is_rejected_with_the_line() {
    for ttl in [599, 86_401, 0] {
        let mut records = vec![want(7, "A", "www", "192.0.2.1", ttl)];
        let problems = validate(&mut records);
        assert_eq!(problems.len(), 1, "{ttl}");
        assert!(problems[0].starts_with("line 7: the TTL "), "{problems:?}");
    }
    for ttl in [600, 3600, 86_400] {
        assert!(validate(&mut [want(1, "A", "www", "192.0.2.1", ttl)]).is_empty());
    }
}

#[test]
fn a_and_aaaa_values_must_be_addresses_of_the_right_kind() {
    for (ty, value) in [
        ("A", "not-an-ip"),
        ("A", "Parked"),
        ("A", ""),
        ("A", "2001:db8::1"),
        ("AAAA", "192.0.2.1"),
        ("AAAA", "nope"),
    ] {
        let problems = validate(&mut [want(4, ty, "www", value, 600)]);
        assert_eq!(problems.len(), 1, "{ty} {value:?}: {problems:?}");
        assert!(
            problems[0].starts_with("line 4: the value "),
            "{problems:?}"
        );
    }
    for (ty, value) in [("A", "192.0.2.1"), ("AAAA", "2001:db8::1")] {
        assert!(validate(&mut [want(1, ty, "www", value, 600)]).is_empty());
    }
}

#[test]
fn an_ipv6_address_matches_the_zone_whatever_its_spelling() {
    // The zone holds one spelling; a zone file or an older export may use another.
    let existing = [have("r1", "AAAA", "v6", "2001:db8::1", 600)];
    for spelling in ["2001:DB8::1", "2001:db8:0:0:0:0:0:1", "2001:0db8::0001"] {
        for replace in [false, true] {
            let mut input = vec![want(1, "AAAA", "v6", spelling, 600)];
            assert!(validate(&mut input).is_empty(), "{spelling}");
            let p = plan(input, &existing, replace);
            assert_eq!(
                kinds(group(&p, "AAAA", "v6")),
                ["unchanged"],
                "{spelling} (replace={replace}) is the record the zone already has"
            );
            assert!(p.groups.iter().all(|g| !kinds(g).contains(&"delete")));
        }
    }
    // A different address is still a different record.
    let p = plan(
        vec![want(1, "AAAA", "v6", "2001:db8::2", 600)],
        &existing,
        false,
    );
    assert_eq!(kinds(group(&p, "AAAA", "v6")), ["create"]);
}

#[test]
fn an_empty_target_or_bad_certificate_is_refused_before_any_plan_is_built() {
    for ty in ["CNAME", "MX", "SRV", "HTTPS", "SVCB"] {
        let mut d = want(5, ty, "x", "", 600);
        d.opts.priority = Some(1);
        d.opts.weight = Some(1);
        d.opts.port = Some(1);
        d.opts.service = Some("_s".into());
        d.opts.protocol = Some("_tcp".into());
        d.name = "x".into();
        let problems = validate(&mut [d]);
        assert!(
            problems.iter().any(|p| p.contains("need a target name")),
            "{ty}: {problems:?}"
        );
    }
    // The root target `.` is a real HTTPS/SVCB value.
    let mut https = want(1, "HTTPS", "@", ".", 600);
    https.opts.priority = Some(1);
    assert!(validate(&mut [https]).is_empty());

    let tlsa = |value: &str| {
        let mut d = want(2, "TLSA", "www", value, 600);
        d.opts.port = Some(443);
        d.opts.protocol = Some("_tcp".into());
        d.opts.usage = Some(3);
        d.opts.selector = Some(1);
        d.opts.matching_type = Some(1);
        d
    };
    for bad in ["", "abc", "zz", "d2abde2"] {
        let problems = validate(&mut [tlsa(bad)]);
        assert_eq!(problems.len(), 1, "{bad:?}: {problems:?}");
        assert!(problems[0].starts_with("line 2: "), "{problems:?}");
    }
    assert!(validate(&mut [tlsa("d2abde24")]).is_empty());
}

#[test]
fn an_owner_that_is_not_a_valid_name_stops_the_plan_before_replace_could_delete_anything() {
    let existing = [have("r1", "A", "keep", "192.0.2.9", 600)];
    let long_label = "a".repeat(64);
    // Four 60-byte labels are 244 bytes on the wire; with `example.com.` that is 257,
    // over the 255 a name may take. Three would still fit.
    let too_long = ["a", "b", "c", "d"].map(|c| c.repeat(60)).join(".");
    for bad in [
        "bad..name",
        ".lead",
        "",
        long_label.as_str(),
        too_long.as_str(),
    ] {
        let mut input = vec![want(6, "A", bad, "192.0.2.1", 600)];
        assert!(
            validate(&mut input).is_empty(),
            "{bad:?}: the shape check needs the domain"
        );
        for replace in [false, true] {
            let errors =
                build_plan("example.com", input.clone(), &existing, replace).expect_err(bad);
            assert_eq!(errors.len(), 1, "{bad:?}: {errors:?}");
            assert!(errors[0].starts_with("line 6: "), "{errors:?}");
            assert!(errors[0].contains("example.com"), "{errors:?}");
        }
    }
    for good in [
        "www",
        "@",
        "_dmarc",
        "*",
        "a.b",
        "xn--caf-dma",
        "_sip._tcp.chat",
    ] {
        let mut input = vec![want(1, "A", good, "192.0.2.1", 600)];
        assert!(validate(&mut input).is_empty(), "{good}");
        assert!(
            build_plan("example.com", input, &[], true).is_ok(),
            "{good} is a valid owner"
        );
    }
}

#[test]
fn a_unicode_target_is_compared_as_the_punycode_target_the_api_holds() {
    let existing = [have("r1", "CNAME", "blog", "xn--caf-dma.example.net.", 600)];
    for target in [
        "café.example.net.",
        "CAFÉ.Example.NET.",
        "xn--caf-dma.example.net.",
    ] {
        let mut input = vec![want(1, "CNAME", "blog", target, 600)];
        assert!(validate(&mut input).is_empty(), "{target}");
        assert_eq!(input[0].value, "xn--caf-dma.example.net.", "{target}");
        let p = plan(input, &existing, false);
        let g = group(&p, "CNAME", "blog");
        assert_eq!(kinds(g), ["unchanged"], "{target}");
        assert!(
            g.blocked.is_empty(),
            "{target}: not a second CNAME to refuse"
        );
    }
}

#[test]
fn the_root_target_and_a_trailing_dot_survive_target_normalization() {
    let mut https = want(1, "HTTPS", "@", ".", 600);
    https.opts.priority = Some(1);
    let mut mx = want(2, "MX", "@", "Mail.Example.COM.", 600);
    mx.opts.priority = Some(10);
    let mut input = [https, mx];
    assert!(validate(&mut input).is_empty());
    assert_eq!(input[0].value, ".");
    assert_eq!(input[1].value, "mail.example.com.");
}

#[test]
fn a_bad_address_stops_the_import_before_replace_could_delete_anything() {
    // Validation runs on every record before a plan is built, so with --replace a
    // record the API would refuse cannot take unrelated existing records with it.
    let mut input = vec![want(1, "A", "www", "not-an-ip", 600)];
    assert_eq!(validate(&mut input).len(), 1);
}

#[test]
fn a_unicode_name_is_compared_in_the_punycode_form_the_api_holds() {
    let existing = [have("r1", "A", "xn--caf-dma", "192.0.2.1", 600)];
    for replace in [false, true] {
        let mut input = vec![want(1, "A", "café", "192.0.2.1", 600)];
        assert!(validate(&mut input).is_empty());
        assert_eq!(input[0].name, "xn--caf-dma");
        let p = plan(input, &existing, replace);
        assert_eq!(kinds(group(&p, "A", "xn--caf-dma")), ["unchanged"]);
        assert!(
            p.groups.iter().all(|g| !kinds(g).contains(&"delete")),
            "replace={replace}: an unchanged listing must not delete its own record"
        );
    }
}

#[test]
fn names_are_lower_cased_and_special_labels_are_left_alone() {
    let mut input = [
        want(1, "A", "WWW", "192.0.2.1", 600),
        want(2, "TXT", "_dmarc", "v=DMARC1", 600),
        want(3, "A", "@", "192.0.2.2", 600),
        want(4, "A", "*", "192.0.2.3", 600),
    ];
    assert!(validate(&mut input).is_empty());
    let names: Vec<&str> = input.iter().map(|d| d.name.as_str()).collect();
    assert_eq!(names, ["www", "_dmarc", "@", "*"]);
}

#[test]
fn srv_may_use_port_zero_but_tlsa_may_not() {
    let mut srv = want(1, "SRV", "@", ".", 600);
    srv.opts.priority = Some(0);
    srv.opts.weight = Some(0);
    srv.opts.port = Some(0);
    srv.opts.service = Some("_sip".into());
    srv.opts.protocol = Some("_tcp".into());
    assert!(
        validate(&mut [srv]).is_empty(),
        "the unavailable-service form"
    );

    let mut tlsa = want(1, "TLSA", "www", "d2ab", 600);
    tlsa.opts.port = Some(0);
    tlsa.opts.protocol = Some("_tcp".into());
    tlsa.opts.usage = Some(3);
    tlsa.opts.selector = Some(1);
    tlsa.opts.matching_type = Some(1);
    let problems = validate(&mut [tlsa]);
    assert!(problems[0].contains("port 0"), "{problems:?}");
}

#[test]
fn types_that_cannot_be_written_are_rejected() {
    for ty in ["NS", "SOA", "PTR", "BOGUS"] {
        let problems = validate(&mut [want(2, ty, "@", "x", 600)]);
        assert_eq!(problems.len(), 1, "{ty}");
        assert!(problems[0].contains("cannot be imported"), "{problems:?}");
    }
}

#[test]
fn fields_a_type_needs_are_required_and_those_it_ignores_are_rejected() {
    let missing = |ty: &str| validate(&mut [want(1, ty, "x", "v", 600)]);
    for ty in ["MX", "SRV", "HTTPS", "SVCB", "CAA", "TLSA"] {
        assert_eq!(missing(ty).len(), 1, "{ty} without its fields");
    }
    let mut stray = want(1, "A", "www", "192.0.2.1", 600);
    stray.opts.priority = Some(10);
    let problems = validate(&mut [stray]);
    assert!(problems[0].contains("only valid for"), "{problems:?}");
}

#[test]
fn numbers_out_of_range_are_rejected_instead_of_silently_dropped() {
    let mut mx = want(1, "MX", "@", "mail.example.com.", 600);
    mx.opts.priority = Some(70_000);
    let problems = validate(&mut [mx]);
    assert!(problems[0].contains("priority 70000"), "{problems:?}");

    let mut caa = want(1, "CAA", "@", "ca.example", 600);
    caa.opts.flag = Some(300);
    caa.opts.tag = Some("issue".into());
    assert!(validate(&mut [caa])[0].contains("flag 300"));

    let mut tlsa = want(1, "TLSA", "www", "d2ab", 600);
    tlsa.opts.port = Some(443);
    tlsa.opts.protocol = Some("_tcp".into());
    tlsa.opts.usage = Some(4);
    tlsa.opts.selector = Some(1);
    tlsa.opts.matching_type = Some(1);
    assert!(validate(&mut [tlsa])[0].contains("usage 4"));
}

#[test]
fn service_and_protocol_labels_get_their_underscore() {
    let mut srv = want(1, "SRV", "chat", "sip.example.net.", 600);
    srv.opts.priority = Some(1);
    srv.opts.weight = Some(1);
    srv.opts.port = Some(5060);
    srv.opts.service = Some("sip".into());
    srv.opts.protocol = Some("tcp".into());
    let mut records = [srv];
    assert!(validate(&mut records).is_empty());
    assert_eq!(records[0].opts.service.as_deref(), Some("_sip"));
    assert_eq!(records[0].opts.protocol.as_deref(), Some("_tcp"));
}

#[test]
fn every_problem_is_reported_each_with_its_own_line() {
    let problems = validate(&mut [
        want(3, "A", "a", "192.0.2.1", 100),
        want(4, "A", "b", "192.0.2.2", 600),
        want(5, "PTR", "c", "x", 600),
    ]);
    assert_eq!(problems.len(), 2);
    assert!(problems[0].starts_with("line 3:") && problems[1].starts_with("line 5:"));
}

// ----------------------------------------------------- additive (the default)

#[test]
fn additive_creates_what_is_missing_and_leaves_identical_records_alone() {
    let existing = [have("r1", "A", "www", "192.0.2.1", 600)];
    let p = plan(
        vec![
            want(1, "A", "www", "192.0.2.1", 600),
            want(2, "A", "www", "192.0.2.2", 600),
        ],
        &existing,
        false,
    );
    assert_eq!(kinds(group(&p, "A", "www")), ["unchanged", "create"]);
}

#[test]
fn additive_never_deletes_or_replaces_a_different_value() {
    // The zone has old.mail.com; the input has new.mail.com. Both end up there.
    let mut old = have("r1", "MX", "@", "old.mail.com.", 600);
    old.priority = Some(10);
    let mut new = want(1, "MX", "@", "new.mail.com.", 600);
    new.opts.priority = Some(10);
    let p = plan(vec![new], &[old], false);
    assert_eq!(kinds(group(&p, "MX", "@")), ["create"]);
    assert!(p.groups.iter().all(|g| !kinds(g).contains(&"delete")));
}

#[test]
fn additive_brings_the_ttl_of_a_matching_record_into_line() {
    let existing = [have("r1", "A", "www", "192.0.2.1", 3600)];
    let p = plan(
        vec![want(1, "A", "www", "192.0.2.1", 600)],
        &existing,
        false,
    );
    let g = group(&p, "A", "www");
    assert_eq!(kinds(g), ["replace"]);
    assert!(matches!(&g.actions[0], SetAction::Replace { record_id, .. } if record_id == "r1"));
}

#[test]
fn additive_matches_a_hostname_however_it_is_written() {
    // The API stores and lists hostnames lower-case with a trailing dot.
    let existing = [have("r1", "CNAME", "blog", "Target.Example.NET.", 600)];
    let p = plan(
        vec![want(1, "CNAME", "blog", "target.example.net", 600)],
        &existing,
        false,
    );
    assert_eq!(kinds(group(&p, "CNAME", "blog")), ["unchanged"]);
}

#[test]
fn a_record_repeated_in_the_input_is_ignored_with_a_warning() {
    let p = plan(
        vec![
            want(1, "A", "www", "192.0.2.1", 600),
            want(9, "A", "www", "192.0.2.1", 600),
        ],
        &[],
        false,
    );
    assert_eq!(group(&p, "A", "www").desired.len(), 1);
    assert_eq!(p.warnings.len(), 1);
    assert!(
        p.warnings[0].starts_with("line 9: ignored"),
        "{:?}",
        p.warnings
    );
    assert!(p.warnings[0].contains("line 1"), "{:?}", p.warnings);
}

#[test]
fn ns_and_soa_in_the_zone_are_never_compared_or_removed() {
    let existing = [
        have("ns", "NS", "@", "ns1.example.com.", 3600),
        have("soa", "SOA", "@", "ns1 admin 1 2 3 4 5", 3600),
    ];
    let p = plan(vec![], &existing, true);
    assert!(p.groups.is_empty(), "{p:?}");
}

// ----------------------------------------------------------- one TTL per set

#[test]
fn records_of_one_name_and_type_with_different_ttls_are_refused() {
    let errors = build_plan(
        "example.com",
        vec![
            want(3, "A", "www", "192.0.2.1", 600),
            want(4, "A", "www", "192.0.2.2", 7200),
        ],
        &[],
        false,
    )
    .expect_err("mixed ttls");
    assert_eq!(errors.len(), 1);
    assert!(errors[0].contains("600, 7200"), "{errors:?}");
    assert!(errors[0].contains("line 3, line 4"), "{errors:?}");
}

#[test]
fn the_same_record_twice_with_different_ttls_is_a_conflict_not_a_repeat() {
    let errors = build_plan(
        "example.com",
        vec![
            want(3, "A", "www", "192.0.2.1", 600),
            want(4, "A", "www", "192.0.2.1", 7200),
        ],
        &[],
        false,
    )
    .expect_err("the TTLs disagree");
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].contains("600, 7200"), "{errors:?}");
}

#[test]
fn different_names_or_types_may_have_different_ttls() {
    let p = plan(
        vec![
            want(1, "A", "www", "192.0.2.1", 600),
            want(2, "A", "mail", "192.0.2.2", 7200),
            want(3, "AAAA", "www", "2001:db8::1", 1800),
        ],
        &[],
        false,
    );
    assert_eq!(p.groups.len(), 3);
}

// ------------------------------------------------------------------ --replace

#[test]
fn replace_deletes_what_the_input_does_not_have() {
    let existing = [
        have("r1", "A", "www", "192.0.2.1", 600),
        have("r2", "A", "old", "192.0.2.9", 600),
        have("r3", "TXT", "@", "stale", 600),
    ];
    let p = plan(vec![want(1, "A", "www", "192.0.2.1", 600)], &existing, true);
    assert_eq!(kinds(group(&p, "A", "www")), ["unchanged"]);
    assert_eq!(kinds(group(&p, "A", "old")), ["delete"]);
    assert_eq!(kinds(group(&p, "TXT", "@")), ["delete"]);
}

#[test]
fn replace_swaps_a_changed_value_in_place() {
    let mut old = have("r1", "MX", "@", "old.mail.com.", 600);
    old.priority = Some(10);
    let mut new = want(1, "MX", "@", "new.mail.com.", 600);
    new.opts.priority = Some(10);
    let p = plan(vec![new], &[old], true);
    assert_eq!(kinds(group(&p, "MX", "@")), ["replace"]);
}

#[test]
fn replace_with_an_empty_input_deletes_every_writable_record() {
    let existing = [
        have("r1", "A", "www", "192.0.2.1", 600),
        have("ns", "NS", "@", "ns1.example.com.", 3600),
    ];
    let p = plan(vec![], &existing, true);
    assert_eq!(p.groups.len(), 1);
    assert_eq!(kinds(&p.groups[0]), ["delete"]);
}

// --------------------------------------------------------------- name rules

#[test]
fn a_cname_next_to_existing_records_is_blocked_with_a_reason() {
    let existing = [have("r1", "A", "www", "192.0.2.1", 600)];
    let p = plan(
        vec![want(1, "CNAME", "www", "target.example.net.", 600)],
        &existing,
        false,
    );
    let g = group(&p, "CNAME", "www");
    assert!(g.actions.is_empty());
    assert_eq!(g.blocked.len(), 1);
    assert!(g.blocked[0].1.contains("can't coexist"), "{:?}", g.blocked);
}

#[test]
fn with_replace_a_cname_may_take_the_name_of_a_record_the_import_deletes() {
    // The A record at `www` is not in the input, so --replace removes it first.
    let existing = [have("r1", "A", "www", "192.0.2.1", 600)];
    let p = plan(
        vec![want(1, "CNAME", "www", "target.example.net.", 600)],
        &existing,
        true,
    );
    assert!(group(&p, "CNAME", "www").blocked.is_empty());
    assert_eq!(kinds(group(&p, "A", "www")), ["delete"]);
}

#[test]
fn two_cnames_for_one_name_in_the_input_block_the_second() {
    let p = plan(
        vec![
            want(1, "CNAME", "blog", "a.example.net.", 600),
            want(2, "CNAME", "blog", "b.example.net.", 600),
        ],
        &[],
        false,
    );
    let g = group(&p, "CNAME", "blog");
    assert_eq!(kinds(g), ["create"]);
    assert_eq!(g.blocked.len(), 1);
    assert_eq!(g.blocked[0].0, 1);
}

#[test]
fn srv_records_are_grouped_by_their_full_owner() {
    let srv = |at, name: &str, service: &str| {
        let mut d = want(at, "SRV", name, "sip.example.net.", 600);
        d.opts.priority = Some(1);
        d.opts.weight = Some(1);
        d.opts.port = Some(5060);
        d.opts.service = Some(service.to_owned());
        d.opts.protocol = Some("_tcp".to_owned());
        d
    };
    let p = plan(
        vec![srv(1, "chat", "_sip"), srv(2, "chat", "_xmpp")],
        &[],
        false,
    );
    assert_eq!(
        p.groups.len(),
        2,
        "a different service is a different owner"
    );
    assert!(p.groups.iter().any(|g| g.owner == "_sip._tcp.chat"));
}

#[test]
fn a_zone_record_without_an_id_stops_the_import_before_anything_is_planned() {
    let mut no_id = have("x", "A", "www", "192.0.2.1", 600);
    no_id.record_id = None;
    let errors = build_plan("example.com", vec![], &[no_id], false).expect_err("no id");
    assert!(errors[0].contains("without an ID"), "{errors:?}");
}
