use super::*;
use crate::options::ParseOptions;
use crate::rdata::CustomShape;
use crate::test_support::dedent;

fn options() -> ParseOptions {
    ParseOptions::default()
        .with_origin("example.com.".parse().expect("origin"))
        .with_custom_type("ALIAS", CustomShape::Name)
}

/// Write `zone`, read it back, and require the same records.
fn assert_round_trips(zone: &Zone) {
    let text = zone.to_zone_file();
    let again = crate::parse(&text, &options())
        .map_err(|e| format!("{e}\n---\n{text}"))
        .expect("the written zone parses");
    assert_eq!(
        again.records, zone.records,
        "round trip changed the records:\n{text}"
    );
    if zone.origin.is_some() {
        // The helper's options supply an origin, so only a zone that had one can
        // be checked for getting it back from `$ORIGIN`.
        assert_eq!(
            again.origin, zone.origin,
            "round trip changed the origin:\n{text}"
        );
    }
}

fn read(body: &str) -> Zone {
    crate::parse(
        &format!("$ORIGIN example.com.\n$TTL 3600\n{}", dedent(body)),
        &options(),
    )
    .expect("parses")
}

#[test]
fn writes_one_full_line_per_record_with_relative_owners() {
    let zone = read(
        r#"
        @ MX 10 mail
        www 300 A 192.0.2.1
        "#,
    );
    assert_eq!(
        zone.to_zone_file(),
        dedent(
            r#"
            $ORIGIN example.com.
            @ 3600 IN MX 10 mail.example.com.
            www 300 IN A 192.0.2.1
            "#
        )
    );
}

#[test]
fn round_trips_every_type() {
    assert_round_trips(&read(
        r#"
        @ SOA ns1 admin 2026 7200 900 1209600 300
        @ NS ns1
        @ A 192.0.2.1
        @ AAAA 2001:db8::1
        www CNAME @
        @ MX 10 mail
        @ MX 20 mail2
        @ TXT "v=spf1 -all"
        @ CAA 0 issue "letsencrypt.org"
        _sip._tcp SRV 10 60 5060 sip
        _443._tcp.www TLSA 3 1 1 d2abde240d7cd3ee6b4b28c54df034b97983a1d16e8a410e4561cb106618e971
        @ HTTPS 1 . alpn=h2,h3
        svc SVCB 0 other.example.net.
        @ ALIAS target.example.net.
        "#,
    ));
}

#[test]
fn txt_with_hard_characters_round_trips() {
    for value in [
        "plain".to_owned(),
        "has \"quotes\" and \\ backslash".to_owned(),
        "semi;colon (paren) $dollar @at".to_owned(),
        "tab\there and newline\nhere".to_owned(),
        "unicode é ✓".to_owned(),
        String::new(),
        "x".repeat(600),
    ] {
        let zone = Zone {
            origin: Some("example.com.".parse().expect("origin")),
            records: vec![Record {
                name: "t.example.com.".parse().expect("name"),
                ttl: 60,
                line: 0,
                data: RData::Txt(if value.is_empty() {
                    vec![Vec::new()]
                } else {
                    value.as_bytes().chunks(255).map(<[u8]>::to_vec).collect()
                }),
            }],
        };
        assert_round_trips(&zone);
    }
}

#[test]
fn txt_with_non_utf8_bytes_round_trips() {
    let zone = Zone {
        origin: None,
        records: vec![Record {
            name: "t.example.com.".parse().expect("name"),
            ttl: 60,
            line: 0,
            data: RData::Txt(vec![vec![0xff, 0xfe, b'"', b'\\', 0x00, b'a']]),
        }],
    };
    assert_round_trips(&zone);
}

#[test]
fn names_outside_the_origin_are_written_absolute() {
    let zone = Zone {
        origin: Some("example.com.".parse().expect("origin")),
        records: vec![Record {
            name: "other.example.org.".parse().expect("name"),
            ttl: 60,
            line: 0,
            data: RData::A("192.0.2.1".parse().expect("ip")),
        }],
    };
    assert!(
        zone.to_zone_file()
            .contains("\nother.example.org. 60 IN A 192.0.2.1\n")
    );
    assert_round_trips(&zone);
}

#[test]
fn zone_without_an_origin_writes_absolute_names_and_no_directive() {
    let zone = Zone {
        origin: None,
        records: vec![Record {
            name: "www.example.com.".parse().expect("name"),
            ttl: 60,
            line: 0,
            data: RData::Cname("example.com.".parse().expect("name")),
        }],
    };
    assert_eq!(
        zone.to_zone_file(),
        "www.example.com. 60 IN CNAME example.com.\n"
    );
    assert_eq!(zone.to_string(), zone.to_zone_file());
}

#[test]
fn quote_escapes_only_what_must_be() {
    assert_eq!(quote(b"a b"), "\"a b\"");
    assert_eq!(quote(b"a\"b\\c"), "\"a\\\"b\\\\c\"");
    assert_eq!(quote(b"a\nb"), "\"a\\010b\"");
    assert_eq!(quote("é".as_bytes()), "\"é\"");
    assert_eq!(quote(&[0xff]), "\"\\255\"");
}
