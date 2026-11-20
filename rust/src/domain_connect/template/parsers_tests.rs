use super::*;
use crate::domain_connect::template::types::TxtConflictMode;

fn one(kind: RecordKind, input: &str) -> Record {
    parse_record(kind, input).expect("record parses")
}

fn fix_of(err: &cli_engine::CliCoreError) -> String {
    cli_engine::build_error_envelope(err, SYSTEM)
        .fix
        .unwrap_or_default()
}

#[test]
fn txt_data_keeps_its_commas() {
    let record = one(
        RecordKind::Txt,
        "host=_dmarc,data=v=DMARC1; p=none; rua=mailto:a@x.com,mailto:b@x.com,ttl=600",
    );

    assert_eq!(record.host.as_deref(), Some("_dmarc"));
    assert_eq!(
        record.data.as_deref(),
        Some("v=DMARC1; p=none; rua=mailto:a@x.com,mailto:b@x.com")
    );
    assert_eq!(record.ttl, Some(Number::Literal(600)));
}

#[test]
fn a_literal_address_is_not_turned_into_a_variable() {
    assert_eq!(
        one(RecordKind::A, "value=192.0.2.1").points_to.as_deref(),
        Some("192.0.2.1")
    );
    assert_eq!(
        one(RecordKind::Aaaa, "value=2001:db8::1")
            .points_to
            .as_deref(),
        Some("2001:db8::1")
    );
}

#[test]
fn a_bare_word_in_an_address_is_a_variable_name() {
    assert_eq!(
        one(RecordKind::A, "value=ip").points_to.as_deref(),
        Some("%ip%")
    );
    assert_eq!(
        one(RecordKind::A, "value=%IP4%").points_to.as_deref(),
        Some("%IP4%")
    );
    assert_eq!(
        one(RecordKind::Cname, "host=www,value=ip")
            .points_to
            .as_deref(),
        Some("ip")
    );
}

#[test]
fn apexcname_honors_host_and_ttl() {
    let given = one(
        RecordKind::Apexcname,
        "host=www,value=target.example.com,ttl=600",
    );
    assert_eq!(given.record_type, "APEXCNAME");
    assert_eq!(given.host.as_deref(), Some("www"));
    assert_eq!(given.points_to.as_deref(), Some("target.example.com"));
    assert_eq!(given.ttl, Some(Number::Literal(600)));

    let defaults = one(RecordKind::Apexcname, "value=target.example.com");
    assert_eq!(defaults.host.as_deref(), Some("@"));
    assert_eq!(defaults.ttl, Some(Number::Literal(3600)));
}

#[test]
fn defaults_fill_in_what_is_left_out() {
    let record = one(RecordKind::Mx, "value=mail.test.com");

    assert_eq!(record.record_type, "MX");
    assert_eq!(record.host.as_deref(), Some("@"));
    assert_eq!(record.points_to.as_deref(), Some("mail.test.com"));
    assert_eq!(record.priority, Some(Number::Literal(10)));
    assert_eq!(record.ttl, Some(Number::Literal(3600)));
    assert_eq!(record.group_id, None);
    assert_eq!(record.essential, None);
}

#[test]
fn srv_reads_every_field() {
    let record = one(
        RecordKind::Srv,
        "service=_sip,protocol=_tcp,target=sip.test.com,priority=20,weight=5,port=5060,ttl=7200",
    );

    assert_eq!(record.service.as_deref(), Some("_sip"));
    assert_eq!(record.protocol.as_deref(), Some("_tcp"));
    assert_eq!(record.name.as_deref(), Some("@"));
    assert_eq!(record.target.as_deref(), Some("sip.test.com"));
    assert_eq!(record.priority, Some(Number::Literal(20)));
    assert_eq!(record.weight, Some(Number::Literal(5)));
    assert_eq!(record.port, Some(Number::Literal(5060)));
    assert_eq!(record.ttl, Some(Number::Literal(7200)));
    assert_eq!(record.host, None);
}

#[test]
fn redirects_have_a_target_and_no_ttl() {
    let record = one(RecordKind::Redir301, "target=https://new.example.com");
    assert_eq!(record.host.as_deref(), Some("@"));
    assert_eq!(record.target.as_deref(), Some("https://new.example.com"));
    assert_eq!(record.ttl, None);

    let err = parse_record(RecordKind::Redir301, "target=https://new.example.com,ttl=5")
        .expect_err("redirects take no ttl");
    assert!(err.to_string().contains("'ttl=5'"), "{err}");
}

#[test]
fn a_required_attribute_must_be_given() {
    let missing = parse_record(RecordKind::Srv, "service=_sip").expect_err("no protocol");
    assert!(
        missing.to_string().contains("requires 'protocol'"),
        "{missing}"
    );

    let empty = parse_record(RecordKind::Mx, "value=").expect_err("empty value");
    assert!(empty.to_string().contains("requires 'value'"), "{empty}");
}

#[test]
fn an_unknown_attribute_is_rejected_and_the_fix_lists_what_is_accepted() {
    let err = parse_record(RecordKind::A, "host=www,tll=600").expect_err("typo");

    assert!(err.to_string().contains("'tll=600'"), "{err}");
    let fix = fix_of(&err);
    assert!(fix.contains("--add-a"), "{fix}");
    assert!(fix.contains("host, value, ttl, group, essential"), "{fix}");
}

#[test]
fn a_part_without_a_key_is_rejected_outside_free_text() {
    let err = parse_record(RecordKind::A, "host=www,oops,value=ip").expect_err("no key");
    assert!(err.to_string().contains("'oops'"), "{err}");
}

#[test]
fn a_repeated_attribute_is_rejected() {
    let err = parse_record(RecordKind::A, "host=a,host=b,value=ip").expect_err("repeated");
    assert!(err.to_string().contains("more than once"), "{err}");
}

#[test]
fn spaces_around_keys_and_values_are_ignored() {
    let record = one(RecordKind::A, "host = www , value = 192.0.2.1");
    assert_eq!(record.host.as_deref(), Some("www"));
    assert_eq!(record.points_to.as_deref(), Some("192.0.2.1"));
}

#[test]
fn an_empty_host_is_kept_as_the_root() {
    let record = one(RecordKind::A, "host=,value=ip");
    assert_eq!(record.host.as_deref(), Some(""));
}

#[test]
fn essential_accepts_only_its_documented_values() {
    let record = one(RecordKind::Mx, "value=mail.test.com,essential=OnApply");
    assert_eq!(record.essential, Some(Essential::OnApply));

    let err =
        parse_record(RecordKind::Mx, "value=mail.test.com,essential=No").expect_err("not a value");
    assert!(err.to_string().contains("'No'"), "{err}");
    assert!(fix_of(&err).contains("Always, OnApply"), "{}", fix_of(&err));
}

#[test]
fn txt_conflict_mode_accepts_only_its_documented_values() {
    let record = one(
        RecordKind::Txt,
        "data=v=spf1 ~all,mode=Prefix,prefix=v=spf1",
    );
    assert_eq!(record.data.as_deref(), Some("v=spf1 ~all"));
    assert_eq!(
        record.txt_conflict_matching_mode,
        Some(TxtConflictMode::Prefix)
    );
    assert_eq!(
        record.txt_conflict_matching_prefix.as_deref(),
        Some("v=spf1")
    );

    let err = parse_record(RecordKind::Txt, "data=x,mode=Append").expect_err("not a mode");
    assert!(
        fix_of(&err).contains("None, All, Prefix"),
        "{}",
        fix_of(&err)
    );
}

#[test]
fn numbers_must_be_whole_numbers() {
    let err = parse_record(RecordKind::A, "value=ip,ttl=soon").expect_err("not a number");
    assert!(
        err.to_string().contains("ttl must be a whole number"),
        "{err}"
    );
    assert!(fix_of(&err).contains("percent signs"), "{}", fix_of(&err));

    for malformed in ["ttl=%", "ttl=%%", "ttl=%a%b%", "ttl=600s"] {
        parse_record(RecordKind::A, &format!("value=ip,{malformed}"))
            .expect_err("neither a number nor a variable");
    }
}

#[test]
fn a_number_can_be_a_variable_and_is_written_as_text() {
    let record = one(
        RecordKind::Srv,
        "service=_sip,protocol=_tcp,target=sip.test.com,port=%port%,ttl=%ttl%",
    );

    assert_eq!(record.port, Some(Number::Variable("%port%".to_owned())));
    let written = serde_json::to_value(&record).expect("serialize");
    assert_eq!(written["port"], "%port%");
    assert_eq!(written["ttl"], "%ttl%");
    assert_eq!(written["priority"], 10);
}

#[test]
fn flag_wide_group_and_essential_apply_unless_a_record_sets_its_own() {
    let inputs = [
        "value=ip".to_owned(),
        "value=ip,group=own,essential=OnApply".to_owned(),
    ];

    let records = parse_records(
        RecordKind::A,
        &inputs,
        Some("shared"),
        Some(Essential::Always),
    )
    .expect("records parse");

    assert_eq!(records.len(), 2);
    assert_eq!(records[0].group_id.as_deref(), Some("shared"));
    assert_eq!(records[0].essential, Some(Essential::Always));
    assert_eq!(records[1].group_id.as_deref(), Some("own"));
    assert_eq!(records[1].essential, Some(Essential::OnApply));
}
