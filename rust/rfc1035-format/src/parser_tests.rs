use super::*;
use crate::rdata::{CustomShape, CustomValue, RData};
use crate::test_support::dedent;

const PRE: &str = "$ORIGIN example.com.\n$TTL 3600\n";

/// Parse `body` after an origin and a default TTL, so its first line is line 3.
fn zone(body: &str) -> Zone {
    parse(&format!("{PRE}{}", dedent(body)), &ParseOptions::default()).expect("parses")
}

fn fail(body: &str) -> ParseError {
    parse(&format!("{PRE}{}", dedent(body)), &ParseOptions::default()).expect_err("should fail")
}

/// Parse a complete zone file written as an indented literal.
fn parse_text(text: &str, options: &ParseOptions) -> Result<Zone, ParseError> {
    parse(&dedent(text), options)
}

fn name(text: &str) -> Name {
    text.parse().expect("name")
}

#[test]
fn at_resolves_to_the_origin_in_every_name_valued_field() {
    let z = zone(
        r#"
        www CNAME @
        @ MX 10 @
        @ NS @
        _s._tcp SRV 1 2 3 @
        @ HTTPS 1 @
        @ SOA @ @ 1 2 3 4 5
        "#,
    );
    let apex = name("example.com.");
    assert_eq!(z.records[0].data, RData::Cname(apex.clone()));
    assert_eq!(
        z.records[1].data,
        RData::Mx {
            preference: 10,
            exchange: apex.clone()
        }
    );
    assert_eq!(z.records[2].data, RData::Ns(apex.clone()));
    assert!(matches!(&z.records[3].data, RData::Srv { target, .. } if *target == apex));
    assert!(matches!(&z.records[4].data, RData::Https(s) if s.target == apex));
    assert!(matches!(&z.records[5].data, RData::Soa(s) if s.mname == apex && s.rname == apex));
}

#[test]
fn relative_targets_get_the_origin() {
    let z = zone(
        r#"
        @ MX 10 mail
        www CNAME web.cdn.example.net.
        "#,
    );
    assert!(
        matches!(&z.records[0].data, RData::Mx { exchange, .. } if *exchange == name("mail.example.com."))
    );
    assert_eq!(
        z.records[1].data,
        RData::Cname(name("web.cdn.example.net."))
    );
}

#[test]
fn txt_joins_parenthesised_and_adjacent_strings_without_stray_quotes() {
    let z = zone(
        r#"
        m TXT ( "a"
                "b" )
        n TXT "a" "b"
        "#,
    );
    let expected = RData::Txt(vec![b"a".to_vec(), b"b".to_vec()]);
    assert_eq!(z.records[0].data, expected);
    assert_eq!(z.records[1].data, expected);
}

#[test]
fn txt_escapes_and_unquoted_words() {
    let z = zone(
        r#"
        t TXT "a\"b;c"
        u TXT plain
        v TXT ""
        "#,
    );
    assert_eq!(z.records[0].data, RData::Txt(vec![b"a\"b;c".to_vec()]));
    assert_eq!(z.records[1].data, RData::Txt(vec![b"plain".to_vec()]));
    assert_eq!(z.records[2].data, RData::Txt(vec![Vec::new()]));
}

#[test]
fn txt_over_255_bytes_is_split_into_strings() {
    let long = "x".repeat(300);
    let z = zone(&format!("l TXT \"{long}\"\n"));
    let parts: &[Vec<u8>] = match &z.records[0].data {
        RData::Txt(parts) => parts,
        _ => &[],
    };
    assert_eq!(parts.iter().map(Vec::len).collect::<Vec<_>>(), [255, 45]);
    assert_eq!(parts.concat(), long.as_bytes());
}

#[test]
fn owner_inheritance_ttl_and_class_in_either_order() {
    let z = zone(
        r#"
        www 600 IN A 192.0.2.1
            IN 7200 A 192.0.2.2
            A 192.0.2.3
        mail A 192.0.2.4
        "#,
    );
    // A record with no TTL takes `$TTL` (RFC 2308 §4), not the TTL of the line above.
    assert_eq!(
        z.records.iter().map(|r| r.ttl).collect::<Vec<_>>(),
        [600, 7200, 3600, 3600]
    );
    let www = name("www.example.com.");
    assert!(z.records[..3].iter().all(|r| r.name == www));
    assert_eq!(z.records[3].name, name("mail.example.com."));
}

#[test]
fn ttl_falls_back_from_explicit_to_dollar_ttl_to_previous_record() {
    let z = parse_text(
        r#"
        $ORIGIN example.com.
        a 90 A 192.0.2.1
        b A 192.0.2.2
        $TTL 2h
        c A 192.0.2.3
        "#,
        &ParseOptions::default(),
    )
    .expect("parses");
    assert_eq!(
        z.records.iter().map(|r| r.ttl).collect::<Vec<_>>(),
        [90, 90, 7200]
    );
}

#[test]
fn missing_ttl_is_an_error_unless_a_default_is_given() {
    let input = r#"
        $ORIGIN example.com.
        a A 192.0.2.1
        "#;
    let err = parse_text(input, &ParseOptions::default()).expect_err("no ttl");
    assert_eq!(err.line(), 2);
    let z = parse_text(input, &ParseOptions::default().with_default_ttl(300)).expect("default");
    assert_eq!(z.records[0].ttl, 300);
}

#[test]
fn origin_option_and_directive() {
    let opts = ParseOptions::default().with_origin(name("example.com."));
    let z = parse_text(
        r#"
        $TTL 60
        www A 192.0.2.1
        $ORIGIN sub.example.com.
        www A 192.0.2.2
        "#,
        &opts,
    )
    .expect("parses");
    assert_eq!(z.origin, Some(name("example.com.")));
    assert_eq!(z.records[0].name, name("www.example.com."));
    assert_eq!(z.records[1].name, name("www.sub.example.com."));

    let err = parse_text(
        r#"
        $TTL 60
        www A 192.0.2.1
        "#,
        &ParseOptions::default(),
    )
    .expect_err("no origin");
    assert_eq!(err.line(), 2);
}

#[test]
fn first_origin_directive_becomes_the_zone_origin() {
    let z = zone("a A 192.0.2.1\n");
    assert_eq!(z.origin, Some(name("example.com.")));
}

#[test]
fn reads_each_built_in_type() {
    let z = zone(
        r#"
        a A 192.0.2.1
        a AAAA 2001:db8::1
        @ NS ns1
        @ CAA 128 issue "letsencrypt.org"
        _443._tcp.www TLSA 3 1 1 D2ABde24 0f
        @ HTTPS 1 . alpn=h2,h3 ech="abc"
        svc SVCB 0 other.example.net.
        @ SOA ns1 admin 2026 2h 15m 2w 5m
        "#,
    );
    assert_eq!(
        z.records[0].data,
        RData::A("192.0.2.1".parse().expect("ip"))
    );
    assert_eq!(
        z.records[1].data,
        RData::Aaaa("2001:db8::1".parse().expect("ip"))
    );
    assert_eq!(
        z.records[3].data,
        RData::Caa {
            flags: 128,
            tag: "issue".into(),
            value: b"letsencrypt.org".to_vec()
        }
    );
    assert_eq!(
        z.records[4].data,
        RData::Tlsa {
            usage: 3,
            selector: 1,
            matching_type: 1,
            data: vec![0xd2, 0xab, 0xde, 0x24, 0x0f]
        }
    );
    assert!(matches!(
        &z.records[5].data,
        RData::Https(s) if s.priority == 1 && s.target.is_root() && s.params == "alpn=h2,h3 ech=\"abc\""
    ));
    assert!(matches!(
        &z.records[6].data,
        RData::Svcb(s) if s.priority == 0 && s.params.is_empty()
    ));
    assert!(matches!(
        &z.records[7].data,
        RData::Soa(s) if (s.serial, s.refresh, s.retry, s.expire, s.minimum)
            == (2026, 7200, 900, 1_209_600, 300)
    ));
}

#[test]
fn type_and_class_are_case_insensitive() {
    let z = zone(
        r#"
        a in a 192.0.2.1
        b CnAmE a
        "#,
    );
    assert_eq!(z.records.len(), 2);
}

#[test]
fn custom_types_are_read_with_their_shape() {
    let opts = ParseOptions::default()
        .with_custom_type("alias", CustomShape::Name)
        .with_custom_type("NOTE", CustomShape::Words);
    let z = parse_text(
        r#"
        $ORIGIN example.com.
        $TTL 600
        @ ALIAS target
        x NOTE "hello world" more
        @ ALIAS @
        "#,
        &opts,
    )
    .expect("parses");
    assert_eq!(z.records[0].data.type_name(), "ALIAS");
    assert_eq!(
        z.records[0].data,
        RData::Custom {
            type_name: "ALIAS".into(),
            value: CustomValue::Name(name("target.example.com."))
        }
    );
    assert_eq!(
        z.records[1].data,
        RData::Custom {
            type_name: "NOTE".into(),
            value: CustomValue::Words(vec!["\"hello world\"".into(), "more".into()])
        }
    );
    assert_eq!(
        z.records[2].data,
        RData::Custom {
            type_name: "ALIAS".into(),
            value: CustomValue::Name(name("example.com."))
        },
        "@ is resolved in a name-shaped custom type too"
    );
}

#[test]
fn unregistered_types_are_an_error_with_a_hint() {
    let err = fail("@ ALIAS target.example.net.\n");
    assert_eq!(err.kind(), ErrorKind::UnknownType);
    assert_eq!((err.line(), err.column()), (3, 3));
    assert!(err.message().contains("with_custom_type"));
}

#[test]
fn built_in_and_malformed_custom_names_are_refused() {
    for bad in ["A", "mx", "", "has space", "123"] {
        let opts = ParseOptions::default().with_custom_type(bad, CustomShape::Name);
        let err = parse("", &opts).expect_err(bad);
        assert_eq!(err.kind(), ErrorKind::InvalidValue, "{bad:?}");
    }
}

#[test]
fn unsupported_features_are_named() {
    for (body, needle) in [
        ("$INCLUDE other.zone\n", "$INCLUDE"),
        ("$GENERATE 1-4 h$ A 10.0.0.$\n", "$GENERATE"),
        ("a CH A 192.0.2.1\n", "class `CH`"),
        ("a TYPE65534 abc\n", "TYPE65534"),
        ("a TYPE65534 \\# 1 00\n", "RFC 3597"),
        ("a A \\# 4 c0000201\n", "RFC 3597"),
    ] {
        let err = fail(body);
        assert_eq!(err.kind(), ErrorKind::Unsupported, "{body:?}");
        assert!(err.message().contains(needle), "{body:?}: {err}");
    }
}

#[test]
fn rejects_bad_values_with_the_position_of_the_value() {
    for (body, line, column) in [
        ("www A not-an-ip\n", 3, 7),
        ("www A 192.0.2.1 extra\n", 3, 17),
        ("@ MX 70000 mail\n", 3, 6),
        ("@ MX mail\n", 3, 6),
        ("@ MX 10\n", 3, 3),
        ("_s._tcp SRV 1 2 70000 t\n", 3, 17),
        ("@ CAA 0 is-sue \"x\"\n", 3, 9),
        ("@ TLSA 3 1 1 abc\n", 3, 14),
        ("@ TLSA 3 1 1 zz\n", 3, 14),
        ("www 5x A 192.0.2.1\n", 3, 5),
        ("www\n", 3, 1),
    ] {
        let err = fail(body);
        assert_eq!(
            (err.line(), err.column()),
            (line, column),
            "{body:?}: {err}"
        );
    }
}

#[test]
fn an_error_is_reported_on_the_line_it_is_found_on() {
    let err = fail(
        r#"
        www A 192.0.2.1 ;ok
        bad..name A 192.0.2.1
        "#,
    );
    assert_eq!((err.line(), err.column()), (4, 1));
}

#[test]
fn duplicate_ttl_and_class_are_rejected() {
    for body in ["x 1 2 A 1.2.3.4\n", "x IN IN A 1.2.3.4\n"] {
        assert_eq!(fail(body).kind(), ErrorKind::Syntax, "{body:?}");
    }
}

#[test]
fn leading_indented_record_has_no_owner() {
    let err = parse_text(
        r#"
        $TTL 60
          A 192.0.2.1
        "#,
        &ParseOptions::default().with_origin(name("a.b.")),
    )
    .expect_err("no owner");
    assert_eq!(err.line(), 2);
}

#[test]
fn directive_errors() {
    assert!(parse("$ORIGIN\n", &ParseOptions::default()).is_err());
    assert!(parse("$TTL\n", &ParseOptions::default()).is_err());
    assert!(parse("$TTL 1x\n", &ParseOptions::default()).is_err());
    assert!(parse("$BOGUS 1\n", &ParseOptions::default()).is_err());
    assert!(parse("$ORIGIN relative\n", &ParseOptions::default()).is_err());
}

#[test]
fn limits_are_enforced() {
    let opts = ParseOptions::default().with_max_records(2);
    let err = parse_text(
        r#"
        $ORIGIN a.b.
        $TTL 60
        x A 1.1.1.1
        y A 1.1.1.2
        z A 1.1.1.3
        "#,
        &opts,
    )
    .expect_err("too many");
    assert_eq!((err.kind(), err.line()), (ErrorKind::Limit, 5));

    let opts = ParseOptions::default().with_max_input_bytes(10);
    assert_eq!(
        parse(&"x".repeat(11), &opts).expect_err("big").kind(),
        ErrorKind::Limit
    );
}

#[test]
fn empty_and_comment_only_input_is_an_empty_zone() {
    let z = parse("; nothing here\n\n", &ParseOptions::default()).expect("parses");
    assert!(z.records.is_empty());
    assert_eq!(z.origin, None);
}

#[test]
fn malformed_input_never_panics() {
    let nasty = [
        "\"",
        "(",
        ")",
        "\\",
        "@",
        "$",
        "$ORIGIN",
        "a \"",
        "a TXT \"\\",
        "a TXT (",
        "é é é",
        "a A \u{0}",
        "a TXT \"\\999\"",
        "\u{feff}a A 1.2.3.4",
        "a\tTXT\t\"x\"\t;",
        "((((",
    ];
    for input in nasty {
        let _ = parse(
            input,
            &ParseOptions::default()
                .with_origin(name("e.com."))
                .with_default_ttl(1),
        );
    }
}
