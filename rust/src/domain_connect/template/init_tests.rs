use crate::domain_connect::template::parsers::*;
use crate::domain_connect::template::types::Template;
use std::collections::HashMap;

#[test]
fn parse_record_attrs_valid_single_pair() {
    let result = parse_record_attrs("host=www").expect("parse failed");
    assert_eq!(result.get("host"), Some(&"www".to_string()));
    assert_eq!(result.len(), 1);
}

#[test]
fn parse_record_attrs_valid_multiple_pairs() {
    let result = parse_record_attrs("host=@,value=mail.com,priority=10").expect("parse failed");
    assert_eq!(result.get("host"), Some(&"@".to_string()));
    assert_eq!(result.get("value"), Some(&"mail.com".to_string()));
    assert_eq!(result.get("priority"), Some(&"10".to_string()));
    assert_eq!(result.len(), 3);
}

#[test]
fn parse_record_attrs_with_spaces() {
    let result = parse_record_attrs("host = www , value = 192.0.2.1").expect("parse failed");
    assert_eq!(result.get("host"), Some(&"www".to_string()));
    assert_eq!(result.get("value"), Some(&"192.0.2.1".to_string()));
}

#[test]
fn parse_record_attrs_empty_value() {
    let result = parse_record_attrs("host=,value=test").expect("parse failed");
    assert_eq!(result.get("host"), Some(&"".to_string()));
    assert_eq!(result.get("value"), Some(&"test".to_string()));
}

#[test]
fn parse_record_attrs_value_with_equals() {
    let result = parse_record_attrs("data=v=spf1 ~all").expect("parse failed");
    assert_eq!(result.get("data"), Some(&"v=spf1 ~all".to_string()));
}

#[test]
fn parse_record_attrs_invalid_no_equals() {
    let result = parse_record_attrs("host");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("Invalid format"));
}

#[test]
fn parse_record_attrs_invalid_multiple_no_equals() {
    let result = parse_record_attrs("host=www,invalid,value=test");
    assert!(result.is_err());
}

#[test]
fn get_optional_attr_returns_value() {
    let mut attrs = HashMap::new();
    attrs.insert("host".to_string(), "www".to_string());
    let result = get_optional_attr(&attrs, "host", "@");
    assert_eq!(result, "www");
}

#[test]
fn get_optional_attr_returns_default() {
    let attrs = HashMap::new();
    let result = get_optional_attr(&attrs, "host", "@");
    assert_eq!(result, "@");
}

#[test]
fn get_required_attr_returns_value() {
    let mut attrs = HashMap::new();
    attrs.insert("value".to_string(), "192.0.2.1".to_string());
    let result = get_required_attr(&attrs, "value", "A").expect("should succeed");
    assert_eq!(result, "192.0.2.1");
}

#[test]
fn get_required_attr_errors_when_missing() {
    let attrs = HashMap::new();
    let result = get_required_attr(&attrs, "value", "A");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.to_string()
            .contains("A record requires 'value' attribute")
    );
}

#[test]
fn get_optional_i32_parses_valid_number() {
    let mut attrs = HashMap::new();
    attrs.insert("ttl".to_string(), "7200".to_string());
    let result = get_optional_i32(&attrs, "ttl", 3600).expect("should succeed");
    assert_eq!(result, 7200);
}

#[test]
fn get_optional_i32_returns_default() {
    let attrs = HashMap::new();
    let result = get_optional_i32(&attrs, "ttl", 3600).expect("should succeed");
    assert_eq!(result, 3600);
}

#[test]
fn get_optional_i32_errors_on_invalid_number() {
    let mut attrs = HashMap::new();
    attrs.insert("ttl".to_string(), "not-a-number".to_string());
    let result = get_optional_i32(&attrs, "ttl", 3600);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("Invalid number"));
}

#[test]
fn add_mx_records_with_all_fields() {
    let mut template = Template::from_basic_inputs(
        "test.com".to_string(),
        "Test".to_string(),
        "email".to_string(),
        "Email".to_string(),
        None,
    );

    let records =
        vec!["value=mail1.test.com,priority=10,ttl=7200,group=mail1,essential=OnApply".to_string()];

    add_mx_records(&mut template, &records, &None, &None).expect("should succeed");

    assert_eq!(template.records.len(), 1);
    let record = &template.records[0];
    assert_eq!(record.record_type, "MX");
    assert_eq!(record.points_to, Some("mail1.test.com".to_string()));
    assert_eq!(record.priority, Some(10));
    assert_eq!(record.ttl, Some(7200));
    assert_eq!(record.group_id, Some("mail1".to_string()));
    assert_eq!(record.essential, Some("OnApply".to_string()));
}

#[test]
fn add_mx_records_with_defaults() {
    let mut template = Template::from_basic_inputs(
        "test.com".to_string(),
        "Test".to_string(),
        "email".to_string(),
        "Email".to_string(),
        None,
    );

    let records = vec!["value=mail.test.com".to_string()];
    add_mx_records(&mut template, &records, &None, &None).expect("should succeed");

    let record = &template.records[0];
    assert_eq!(record.host, Some("@".to_string()));
    assert_eq!(record.priority, Some(10));
    assert_eq!(record.ttl, Some(3600));
}

#[test]
fn add_srv_records_with_all_fields() {
    let mut template = Template::from_basic_inputs(
        "test.com".to_string(),
        "Test".to_string(),
        "voip".to_string(),
        "VoIP".to_string(),
        None,
    );

    let records = vec![
        "service=_sip,protocol=_tcp,target=sip.test.com,priority=10,weight=5,port=5060,ttl=7200"
            .to_string(),
    ];

    add_srv_records(&mut template, &records, &None, &None).expect("should succeed");

    assert_eq!(template.records.len(), 1);
    let record = &template.records[0];
    assert_eq!(record.record_type, "SRV");
    assert_eq!(record.service, Some("_sip".to_string()));
    assert_eq!(record.protocol, Some("_tcp".to_string()));
    assert_eq!(record.target, Some("sip.test.com".to_string()));
    assert_eq!(record.priority, Some(10));
    assert_eq!(record.weight, Some(5));
    assert_eq!(record.port, Some(5060));
    assert_eq!(record.ttl, Some(7200));
}

#[test]
fn add_srv_records_missing_required_fields() {
    let mut template = Template::from_basic_inputs(
        "test.com".to_string(),
        "Test".to_string(),
        "voip".to_string(),
        "VoIP".to_string(),
        None,
    );

    let records = vec!["service=_sip".to_string()];
    let result = add_srv_records(&mut template, &records, &None, &None);
    assert!(result.is_err());
}

#[test]
fn add_txt_records_with_conflict_fields() {
    let mut template = Template::from_basic_inputs(
        "test.com".to_string(),
        "Test".to_string(),
        "verify".to_string(),
        "Verify".to_string(),
        None,
    );

    let records = vec!["data=v=spf1 ~all,mode=Append,prefix=v=spf1".to_string()];

    add_txt_records(&mut template, &records, &None, &None).expect("should succeed");

    let record = &template.records[0];
    assert_eq!(record.record_type, "TXT");
    assert_eq!(record.data, Some("v=spf1 ~all".to_string()));
    assert_eq!(
        record.txt_conflict_matching_mode,
        Some("Append".to_string())
    );
    assert_eq!(
        record.txt_conflict_matching_prefix,
        Some("v=spf1".to_string())
    );
}

#[test]
fn add_spfm_records_with_rules() {
    let mut template = Template::from_basic_inputs(
        "test.com".to_string(),
        "Test".to_string(),
        "email".to_string(),
        "Email".to_string(),
        None,
    );

    let records = vec!["rules=include:_spf.test.com".to_string()];
    add_spfm_records(&mut template, &records, &None, &None).expect("should succeed");

    let record = &template.records[0];
    assert_eq!(record.record_type, "SPFM");
    assert_eq!(record.spf_rules, Some("include:_spf.test.com".to_string()));
}

#[test]
fn add_multiple_records_same_type() {
    let mut template = Template::from_basic_inputs(
        "test.com".to_string(),
        "Test".to_string(),
        "email".to_string(),
        "Email".to_string(),
        None,
    );

    let records = vec![
        "value=mail1.test.com,priority=10".to_string(),
        "value=mail2.test.com,priority=20".to_string(),
        "value=mail3.test.com,priority=30".to_string(),
    ];

    add_mx_records(&mut template, &records, &None, &None).expect("should succeed");
    assert_eq!(template.records.len(), 3);

    let priorities: Vec<i32> = template.records.iter().filter_map(|r| r.priority).collect();
    assert_eq!(priorities, vec![10, 20, 30]);
}

#[test]
fn global_group_id_applied_when_not_overridden() {
    let mut template = Template::from_basic_inputs(
        "test.com".to_string(),
        "Test".to_string(),
        "web".to_string(),
        "Web".to_string(),
        None,
    );

    let global_group = Some("global-group".to_string());
    let records = vec!["value=192.0.2.1".to_string()];

    add_a_records(&mut template, &records, &global_group, &None).expect("should succeed");

    let record = &template.records[0];
    assert_eq!(record.group_id, Some("global-group".to_string()));
}

#[test]
fn local_group_id_overrides_global() {
    let mut template = Template::from_basic_inputs(
        "test.com".to_string(),
        "Test".to_string(),
        "web".to_string(),
        "Web".to_string(),
        None,
    );

    let global_group = Some("global-group".to_string());
    let records = vec!["value=192.0.2.1,group=local-group".to_string()];

    add_a_records(&mut template, &records, &global_group, &None).expect("should succeed");

    let record = &template.records[0];
    assert_eq!(record.group_id, Some("local-group".to_string()));
}

#[test]
fn global_essential_applied_when_not_overridden() {
    let mut template = Template::from_basic_inputs(
        "test.com".to_string(),
        "Test".to_string(),
        "web".to_string(),
        "Web".to_string(),
        None,
    );

    let global_essential = Some("Always".to_string());
    let records = vec!["value=192.0.2.1".to_string()];

    add_a_records(&mut template, &records, &None, &global_essential).expect("should succeed");

    let record = &template.records[0];
    assert_eq!(record.essential, Some("Always".to_string()));
}
