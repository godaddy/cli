//! Parses zone files shaped like real exports and checks what comes out, then
//! checks the writer's output reads back to the same records.

use rfc1035_format::{CustomShape, ErrorKind, Name, ParseOptions, RData, Record, Zone, parse};

fn options() -> ParseOptions {
    ParseOptions::default().with_custom_type("ALIAS", CustomShape::Name)
}

fn read(fixture: &str) -> Zone {
    let path = format!("{}/tests/fixtures/{fixture}", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).expect("fixture exists");
    parse(&text, &options())
        .map_err(|e| format!("{fixture}: {e}"))
        .expect("fixture parses")
}

fn name(text: &str) -> Name {
    text.parse().expect("name")
}

fn of_type<'a>(zone: &'a Zone, type_name: &str) -> Vec<&'a Record> {
    zone.records
        .iter()
        .filter(|r| r.data.type_name() == type_name)
        .collect()
}

fn txt_value(record: &Record) -> Vec<u8> {
    match &record.data {
        RData::Txt(parts) => parts.concat(),
        _ => Vec::new(),
    }
}

#[test]
fn bind_style_zone() {
    let zone = read("bind_named_checkzone.zone");
    assert_eq!(zone.origin, Some(name("example.com.")));
    assert_eq!(zone.records.len(), 12);

    let soa = of_type(&zone, "SOA");
    assert!(matches!(
        &soa[0].data,
        RData::Soa(s) if s.serial == 2_026_111_801 && s.refresh == 7200 && s.retry == 900
            && s.expire == 1_209_600 && s.minimum == 300
    ));

    // The records under the SOA inherit the apex as their owner and `$TTL 1h`.
    let apex = name("example.com.");
    let inherited = zone.records.iter().take(6).collect::<Vec<_>>();
    assert!(inherited.iter().all(|r| r.name == apex && r.ttl == 3600));

    assert_eq!(
        of_type(&zone, "CNAME")
            .iter()
            .map(|r| r.data.clone())
            .collect::<Vec<_>>(),
        [RData::Cname(apex), RData::Cname(name("www.example.com."))],
        "`CNAME @` points at the apex and `CNAME www` is relative"
    );
    assert_eq!(of_type(&zone, "A")[2].ttl, 300);
}

#[test]
fn email_provider_records_keep_their_values_exactly() {
    let zone = read("email_providers.zone");
    let txts = of_type(&zone, "TXT");

    let verification = txts
        .iter()
        .find(|r| txt_value(r).starts_with(b"google-site-verification="))
        .expect("verification record present");
    assert_eq!(
        txt_value(verification),
        b"google-site-verification=rXOxyZounnZasA8Z7oaD3c14JdjS9aKSWvsR1EbUSIQ"
    );

    let dkim = txts
        .iter()
        .find(|r| r.name == name("google._domainkey.example.com."))
        .expect("DKIM record present");
    let value = String::from_utf8(txt_value(dkim)).expect("utf-8");
    assert!(value.starts_with("v=DKIM1; k=rsa; p=MIIBIjAN"), "{value}");
    assert!(value.ends_with("IDAQAB"), "{value}");
    assert!(
        !value.contains('"'),
        "quote characters are syntax, not data"
    );
    assert_eq!(
        value.matches("p=").count(),
        1,
        "parts are joined, not interleaved"
    );

    // 255 + 10 characters written as two strings stay two strings, and
    // concatenate to one 265-byte value.
    let long = txts
        .iter()
        .find(|r| r.name == name("long.example.com."))
        .expect("long record present");
    let lengths = match &long.data {
        RData::Txt(parts) => parts.iter().map(Vec::len).collect::<Vec<_>>(),
        _ => Vec::new(),
    };
    assert_eq!(lengths, [255, 10]);
    assert_eq!(txt_value(long).len(), 265);
}

#[test]
fn modern_types_and_the_registered_custom_type() {
    let zone = read("modern_types.zone");
    assert_eq!(of_type(&zone, "CAA").len(), 3);
    assert!(matches!(
        &of_type(&zone, "CAA")[1].data,
        RData::Caa { tag, value, .. } if tag == "issuewild" && value == b";"
    ));
    assert!(matches!(
        &of_type(&zone, "TLSA")[0].data,
        RData::Tlsa { data, .. } if data.len() == 32
    ));
    let https = of_type(&zone, "HTTPS");
    assert!(
        matches!(&https[0].data, RData::Https(s) if s.target.is_root() && s.params == "alpn=h2,h3")
    );
    assert!(matches!(
        &https[1].data,
        RData::Https(s) if s.params == "alpn=\"h2\" port=8443"
    ));
    assert!(matches!(
        &of_type(&zone, "ALIAS")[0].data,
        RData::Custom { .. }
    ));
}

#[test]
fn modern_types_fail_without_registering_the_custom_type() {
    let path = format!(
        "{}/tests/fixtures/modern_types.zone",
        env!("CARGO_MANIFEST_DIR")
    );
    let text = std::fs::read_to_string(path).expect("fixture exists");
    let err = parse(&text, &ParseOptions::default()).expect_err("ALIAS is unknown");
    assert_eq!(err.kind(), ErrorKind::UnknownType);
    assert_eq!(
        err.line(),
        12,
        "the error names the line of the ALIAS record"
    );
}

#[test]
fn every_fixture_round_trips_through_the_writer() {
    for fixture in [
        "bind_named_checkzone.zone",
        "email_providers.zone",
        "modern_types.zone",
    ] {
        let zone = read(fixture);
        let text = zone.to_zone_file();
        let again = parse(&text, &options())
            .map_err(|e| format!("{fixture}: {e}\n{text}"))
            .expect("written zone parses");
        assert_eq!(again, zone, "{fixture} changed on the round trip:\n{text}");
    }
}
