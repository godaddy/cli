use super::*;

use cli_engine::{Cli, CliConfig};
use domains_client::types;
use serde_json::Value;

fn cli() -> Cli {
    Cli::new(
        CliConfig::new("gddy", "GoDaddy developer CLI", "gddy")
            .with_default_auth_provider("godaddy")
            .with_module(crate::dns::module()),
    )
}

/// `dns export` and `dns import` reach the Domains API like the other `dns`
/// commands, so they fail closed at auth, a dry run of an import included.
#[tokio::test]
async fn export_and_import_require_auth() {
    const AUTH_FAILURE_EXIT: i32 = 2;
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("z.zone");
    std::fs::write(&file, "www 600 A 192.0.2.1\n").expect("write");
    let file = file.to_str().expect("utf-8 path").to_owned();
    let cases: Vec<Vec<&str>> = vec![
        vec!["gddy", "dns", "export", "example.com"],
        vec!["gddy", "dns", "export", "example.com", "--format", "json"],
        vec!["gddy", "dns", "import", "example.com", "--file", &file],
        vec![
            "gddy",
            "dns",
            "import",
            "example.com",
            "--file",
            &file,
            "--dry-run",
        ],
        vec![
            "gddy",
            "dns",
            "import",
            "example.com",
            "--file",
            &file,
            "--replace",
            "--dry-run",
        ],
    ];
    for args in cases {
        let output = cli().run(args.iter().copied()).await;
        assert_eq!(
            output.exit_code, AUTH_FAILURE_EXIT,
            "{args:?}: {}",
            output.rendered
        );
    }
}

#[tokio::test]
async fn export_and_import_reject_bad_arguments_at_parse_time() {
    for (args, needle) in [
        (vec!["gddy", "dns", "import", "example.com"], "--file"),
        (
            vec!["gddy", "dns", "export", "example.com", "--format", "yaml"],
            "yaml",
        ),
        (
            vec![
                "gddy",
                "dns",
                "import",
                "example.com",
                "--file",
                "x",
                "--format",
                "csv",
            ],
            "csv",
        ),
    ] {
        let output = cli().run(args.iter().copied()).await;
        assert_ne!(output.exit_code, 0, "{args:?}: {}", output.rendered);
        assert!(
            output.rendered.contains(needle),
            "{args:?}: {}",
            output.rendered
        );
    }
}

fn record(ty: &str, name: &str, data: &str) -> types::DnsRecord {
    serde_json::from_value(json!({
        "type": ty, "name": name, "data": data, "ttl": 600, "recordId": format!("id-{name}")
    }))
    .expect("record")
}

fn exported_types(format: ExportFormat, standard_only: bool) -> Vec<String> {
    let args = ExportArgs {
        domain: "example.com".to_owned(),
        format,
        standard_only,
    };
    let text = render(
        vec![
            record("A", "www", "192.0.2.1"),
            record("DNAME", "@", "target.example.net."),
        ],
        &args,
    )
    .expect("renders");
    let mut types: Vec<String> = match format {
        ExportFormat::Json => serde_json::from_str::<Vec<Value>>(&text)
            .expect("json")
            .iter()
            .filter_map(|r| r["type"].as_str().map(str::to_owned))
            .collect(),
        ExportFormat::Zone => text
            .lines()
            .filter(|l| !l.starts_with(['$', ';']))
            .filter_map(|l| l.split_whitespace().nth(3).map(str::to_owned))
            .collect(),
    };
    // What is exported matters here, not the order a format writes it in.
    types.sort();
    types
}

#[test]
fn standard_only_also_leaves_out_an_a_record_that_has_no_address() {
    let args = |standard_only| ExportArgs {
        domain: "example.com".to_owned(),
        format: ExportFormat::Zone,
        standard_only,
    };
    let records = || {
        vec![
            record("A", "www", "192.0.2.1"),
            record("A", "@", "placeholder"),
        ]
    };

    // Without the flag the export stops, and the error says how to get past it.
    let err = render(records(), &args(false)).expect_err("a non-address can't be written");
    let message = err.to_string();
    assert!(message.contains("A @ placeholder"), "{message}");
    let fix = format!("{err:?}");
    assert!(
        fix.contains("--standard-only"),
        "the hint names the flag: {fix}"
    );

    // With it, only the real address is written, and the placeholder is noted.
    let text = render(records(), &args(true)).expect("renders");
    assert!(text.contains(" IN A 192.0.2.1"), "{text}");
    assert!(text.contains(";   A @ placeholder"), "{text}");
}

#[test]
fn the_error_for_an_unwritable_record_only_suggests_standard_only_when_it_would_help() {
    // A record that fails for another reason is not fixed by leaving out GoDaddy-only
    // records, so the hint is the JSON one.
    let broken_mx = record("MX", "@", "mail.example.com.");
    let args = ExportArgs {
        domain: "example.com".to_owned(),
        format: ExportFormat::Zone,
        standard_only: false,
    };
    let err = render(vec![broken_mx], &args).expect_err("an MX without a priority");
    let fix = format!("{err:?}");
    assert!(fix.contains("--format json"), "{fix}");
    assert!(!fix.contains("--standard-only"), "{fix}");
}

/// What `render` writes for each record, as `type owner` pairs, whichever format.
fn exported_pairs(format: ExportFormat, records: Vec<types::DnsRecord>) -> Vec<String> {
    let args = ExportArgs {
        domain: "example.com".to_owned(),
        format,
        standard_only: false,
    };
    let text = render(records, &args).expect("renders");
    let mut pairs: Vec<String> = match format {
        ExportFormat::Json => serde_json::from_str::<Vec<Value>>(&text)
            .expect("json")
            .iter()
            .map(|r| {
                format!(
                    "{} {}",
                    r["type"].as_str().unwrap_or(""),
                    r["name"].as_str().unwrap_or("")
                )
            })
            .collect(),
        ExportFormat::Zone => text
            .lines()
            .filter(|l| !l.starts_with(['$', ';']))
            .filter_map(|l| {
                let mut words = l.split_whitespace();
                let owner = words.next()?;
                Some(format!("{} {owner}", words.nth(2)?))
            })
            .collect(),
    };
    pairs.sort();
    pairs
}

fn args(format: ExportFormat, standard_only: bool) -> ExportArgs {
    ExportArgs {
        domain: "example.com".to_owned(),
        format,
        standard_only,
    }
}

#[test]
fn without_standard_only_there_is_no_notice_because_an_unusable_record_stops_the_export() {
    let records = vec![
        record("A", "www", "192.0.2.1"),
        record("A", "bad", "placeholder"),
    ];
    for format in [ExportFormat::Zone, ExportFormat::Json] {
        assert!(
            notices(&records, &args(format, false)).is_empty(),
            "{format:?}"
        );
    }
    // The zone format stops and names the record, and the hint offers the flag.
    let err = render(records, &args(ExportFormat::Zone, false)).expect_err("cannot be written");
    assert!(err.to_string().contains("A bad placeholder"), "{err}");
    assert!(format!("{err:?}").contains("--standard-only"), "{err:?}");
}

#[test]
fn with_standard_only_the_notice_says_what_was_left_out() {
    let records = vec![
        record("A", "www", "192.0.2.1"),
        record("DNAME", "@", "target.example.net."),
    ];
    for format in [ExportFormat::Zone, ExportFormat::Json] {
        let notices = notices(&records, &args(format, true));
        assert_eq!(notices.len(), 1, "{format:?}: {notices:?}");
        assert!(
            notices[0].starts_with("Left out 1 record(s)"),
            "{format:?}: {notices:?}"
        );
        assert!(
            notices[0].contains("DNAME @ target.example.net."),
            "{format:?}: {notices:?}"
        );
        assert!(
            !notices[0].contains("--standard-only"),
            "no advice to add the flag it has: {notices:?}"
        );
    }
}

#[test]
fn there_is_no_notice_when_every_record_is_standard_or_provider_managed() {
    let records = vec![
        record("A", "www", "192.0.2.1"),
        record("SOA", "@", ""),
        record("NS", "@", "ns1.example.com."),
        record("A", "@", "Parked"),
    ];
    for standard_only in [false, true] {
        assert!(
            notices(&records, &args(ExportFormat::Zone, standard_only)).is_empty(),
            "the SOA, apex NS and parked marker are never exported, so they are never warned about"
        );
    }
}

#[test]
fn notices_go_to_the_writer_one_per_line_and_a_failed_write_is_ignored() {
    let mut out = Vec::new();
    write_notices(&mut out, &["first".to_owned(), "second".to_owned()]);
    assert_eq!(
        String::from_utf8(out).expect("utf-8"),
        "Note: first\nNote: second\n"
    );

    struct Closed;
    impl Write for Closed {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Err(std::io::ErrorKind::BrokenPipe.into())
        }
    }
    // An export that worked must not fail because stderr is closed.
    write_notices(&mut Closed, &["lost".to_owned()]);
}

#[test]
fn the_soa_and_the_domains_own_ns_records_are_never_exported_but_delegations_are() {
    let records = || {
        vec![
            record("SOA", "@", ""),
            record("NS", "@", "ns1.example.com."),
            record("NS", "@", "ns2.example.com."),
            record("NS", "sub", "ns.other.net."),
            record("A", "www", "192.0.2.1"),
        ]
    };
    for format in [ExportFormat::Zone, ExportFormat::Json] {
        assert_eq!(
            exported_pairs(format, records()),
            ["A www", "NS sub"],
            "{format:?}: the SOA and the domain's own NS records are left out, the delegation stays"
        );
    }
}

#[test]
fn a_parked_domains_marker_is_never_exported_in_either_format() {
    // GoDaddy writes `Parked` as the A value of a parked domain, and the API
    // refuses it from a client, so an export can't carry it. Without the flag.
    let records = || {
        vec![
            record("A", "@", "Parked"),
            record("A", "www", "192.0.2.1"),
            record("AAAA", "v6", "Parked"),
        ]
    };
    for format in [ExportFormat::Zone, ExportFormat::Json] {
        assert_eq!(
            exported_pairs(format, records()),
            ["A www"],
            "{format:?}: the marker is left out and the real address stays"
        );
    }
}

#[test]
fn the_parked_marker_leaves_no_note_and_does_not_need_standard_only() {
    let args = |standard_only| ExportArgs {
        domain: "example.com".to_owned(),
        format: ExportFormat::Zone,
        standard_only,
    };
    for standard_only in [false, true] {
        let text = render(
            vec![record("A", "@", "Parked"), record("A", "www", "192.0.2.1")],
            &args(standard_only),
        )
        .expect("renders without needing the flag");
        assert!(!text.contains("Parked"), "{standard_only}: {text}");
        assert!(
            !text.contains("Left out"),
            "{standard_only}: no note for it: {text}"
        );
        assert!(text.contains(" IN A 192.0.2.1"), "{standard_only}: {text}");
    }
}

#[test]
fn a_domain_with_only_the_parked_marker_exports_an_empty_zone_not_an_error() {
    let args = ExportArgs {
        domain: "example.com".to_owned(),
        format: ExportFormat::Json,
        standard_only: false,
    };
    let text = render(vec![record("A", "@", "Parked")], &args).expect("renders");
    assert_eq!(
        serde_json::from_str::<Vec<Value>>(&text)
            .expect("json")
            .len(),
        0
    );
}

#[test]
fn an_empty_soa_from_the_api_no_longer_matters_to_an_export() {
    // The API returns the SOA with no data. It used to be reported as unreadable;
    // now it is left out with the other records the provider manages.
    let args = ExportArgs {
        domain: "example.com".to_owned(),
        format: ExportFormat::Zone,
        standard_only: false,
    };
    let text = render(
        vec![record("SOA", "@", ""), record("A", "www", "192.0.2.1")],
        &args,
    )
    .expect("renders");
    assert!(!text.contains("SOA"), "{text}");
    assert!(
        !text.contains("Left out"),
        "no note for what is never exported: {text}"
    );
}

#[test]
fn standard_only_leaves_out_a_record_type_the_cli_does_not_know_in_either_format() {
    // JSON carries any type the API returns, so it is the format that can show one.
    assert_eq!(
        exported_types(ExportFormat::Json, false),
        ["A", "DNAME"],
        "an unknown type is exported as it is without the flag"
    );
    for format in [ExportFormat::Zone, ExportFormat::Json] {
        assert_eq!(exported_types(format, true), ["A"], "{format:?}");
    }
}

#[test]
fn a_zone_file_stops_on_a_record_type_it_cannot_write_and_says_how_to_go_on() {
    let err = render(
        vec![
            record("A", "www", "192.0.2.1"),
            record("DNAME", "@", "target.example.net."),
        ],
        &args(ExportFormat::Zone, false),
    )
    .expect_err("a zone file cannot hold a DNAME here");
    assert!(
        err.to_string().contains("DNAME @ target.example.net."),
        "{err}"
    );
    assert!(format!("{err:?}").contains("--standard-only"), "{err:?}");
}

#[test]
fn export_json_is_what_dns_list_prints() {
    let record: types::DnsRecord = serde_json::from_value(json!({
        "type": "A", "name": "www", "data": "192.0.2.1", "ttl": 600, "recordId": "r1"
    }))
    .expect("record");
    let list = build_list_output(std::slice::from_ref(&record)).expect("list");
    assert_eq!(list[0]["data"], "192.0.2.1");
    assert_eq!(list[0]["ttl"], 600);
}
