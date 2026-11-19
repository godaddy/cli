//! Tests for the per-record flags of `dns add` and `dns set`: a flag given once
//! applies to every `--data` value, and one given once per value pairs with them.

use super::tests::{WriteCli, opts};
use super::*;

/// The options `add` and `set` would use for each `--data` value of this command line.
fn per_record_for(record_type: &str, extra: &[&str]) -> Result<Vec<RecordOptions>, String> {
    use clap::Parser;
    let mut argv = vec!["t", "example.com", "--type", record_type, "--name", "n"];
    argv.extend_from_slice(extra);
    let cli = WriteCli::try_parse_from(argv).map_err(|e| e.to_string())?;
    RecordOptions::per_record(&cli.args)
}

#[test]
fn a_flag_given_once_applies_to_every_data_value() {
    let each = per_record_for(
        "MX",
        &[
            "--data",
            "a.example.com",
            "--data",
            "b.example.com",
            "--priority",
            "10",
            "--ttl",
            "900",
        ],
    )
    .expect("valid");
    assert_eq!(each.len(), 2);
    assert!(
        each.iter()
            .all(|o| o.priority == Some(10) && o.ttl == Some(900))
    );
}

#[test]
fn a_flag_given_once_per_data_value_pairs_with_them_in_order() {
    // Google Workspace's mail servers: three priorities in one command.
    let each = per_record_for(
        "MX",
        &[
            "--data",
            "aspmx.l.google.com",
            "--priority",
            "1",
            "--data",
            "alt1.aspmx.l.google.com",
            "--priority",
            "5",
            "--data",
            "alt2.aspmx.l.google.com",
            "--priority",
            "10",
        ],
    )
    .expect("valid");
    let priorities: Vec<_> = each.iter().map(|o| o.priority).collect();
    assert_eq!(priorities, [Some(1), Some(5), Some(10)]);
}

#[test]
fn the_pairing_follows_the_order_of_the_flags_not_where_they_sit_among_the_data() {
    // `--priority` and `--data` are separate lists; only their order within
    // each list matters.
    let each = per_record_for(
        "MX",
        &[
            "--priority",
            "1",
            "--priority",
            "5",
            "--data",
            "a.example.com",
            "--data",
            "b.example.com",
        ],
    )
    .expect("valid");
    assert_eq!(each[0].priority, Some(1));
    assert_eq!(each[1].priority, Some(5));
}

#[test]
fn a_count_that_is_neither_one_nor_the_number_of_data_values_is_rejected() {
    let err = per_record_for(
        "MX",
        &[
            "--data",
            "a.example.com",
            "--data",
            "b.example.com",
            "--data",
            "c.example.com",
            "--priority",
            "1",
            "--priority",
            "5",
        ],
    )
    .expect_err("two priorities for three records");
    assert!(
        err.contains("--priority") && err.contains("2 times") && err.contains("3 --data"),
        "{err}"
    );
    // The same rule for every flag, not just priority.
    let err = per_record_for(
        "CAA",
        &[
            "--data",
            "a.example",
            "--data",
            "b.example",
            "--data",
            "c.example",
            "--tag",
            "issue",
            "--tag",
            "issuewild",
        ],
    )
    .expect_err("two tags for three records");
    assert!(err.contains("--tag"), "{err}");
}

#[test]
fn every_per_record_flag_pairs_the_same_way() {
    let each = per_record_for(
        "CAA",
        &[
            "--data",
            "letsencrypt.org",
            "--tag",
            "issue",
            "--flag",
            "0",
            "--data",
            "letsencrypt.org",
            "--tag",
            "issuewild",
            "--flag",
            "128",
        ],
    )
    .expect("valid");
    assert_eq!(each[0].tag.as_deref(), Some("issue"));
    assert_eq!(each[0].flag, Some(0));
    assert_eq!(each[1].tag.as_deref(), Some("issuewild"));
    assert_eq!(each[1].flag, Some(128));

    let each = per_record_for(
        "SRV",
        &[
            "--data",
            "a.example.com.",
            "--data",
            "b.example.com.",
            "--priority",
            "10",
            "--priority",
            "20",
            "--weight",
            "5",
            "--port",
            "5060",
            "--port",
            "5061",
            "--service",
            "sip",
            "--protocol",
            "tcp",
        ],
    )
    .expect("valid");
    assert_eq!(
        each.iter()
            .map(|o| (o.priority, o.weight, o.port))
            .collect::<Vec<_>>(),
        [
            (Some(10), Some(5), Some(5060)),
            (Some(20), Some(5), Some(5061))
        ]
    );
    assert!(each.iter().all(|o| o.service.as_deref() == Some("_sip")));
}

#[test]
fn a_required_flag_is_still_required_when_the_flags_repeat() {
    let err = per_record_for(
        "MX",
        &["--data", "a.example.com", "--data", "b.example.com"],
    )
    .expect_err("MX needs a priority");
    assert!(err.contains("--priority"), "{err}");
}

#[test]
fn v3_records_gives_each_data_value_its_own_options() {
    let with = |priority| RecordOptions {
        priority: Some(priority),
        ..opts()
    };
    let recs = v3_records(
        "@",
        "MX",
        &["a.example.com".to_owned(), "b.example.com".to_owned()],
        &[with(1), with(5)],
    );
    assert_eq!(
        recs.iter().map(|r| r.priority).collect::<Vec<_>>(),
        [Some(1), Some(5)]
    );
}
