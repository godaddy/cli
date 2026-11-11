//! Record parser functions for Domain Connect templates

use super::types::{Record, Template};
use cli_engine::CliCoreError;
use std::collections::HashMap;

pub(crate) fn parse_record_attrs(input: &str) -> Result<HashMap<String, String>, CliCoreError> {
    let mut attrs = HashMap::new();

    for pair in input.split(',') {
        let parts: Vec<&str> = pair.splitn(2, '=').collect();
        if parts.len() == 2 {
            attrs.insert(parts[0].trim().to_string(), parts[1].trim().to_string());
        } else {
            return Err(CliCoreError::message(format!(
                "Invalid format '{}'. Expected key=value pairs separated by commas",
                pair
            )));
        }
    }

    Ok(attrs)
}

/// Get a required attribute or return an error
pub(crate) fn get_required_attr(
    attrs: &HashMap<String, String>,
    key: &str,
    record_type: &str,
) -> Result<String, CliCoreError> {
    attrs.get(key).cloned().ok_or_else(|| {
        CliCoreError::message(format!(
            "{} record requires '{}' attribute",
            record_type, key
        ))
    })
}

/// Get an optional attribute with a default value
pub(crate) fn get_optional_attr(
    attrs: &HashMap<String, String>,
    key: &str,
    default: &str,
) -> String {
    attrs
        .get(key)
        .cloned()
        .unwrap_or_else(|| default.to_string())
}

/// Get an optional i32 attribute with a default value
pub(crate) fn get_optional_i32(
    attrs: &HashMap<String, String>,
    key: &str,
    default: i32,
) -> Result<i32, CliCoreError> {
    if let Some(val) = attrs.get(key) {
        val.parse()
            .map_err(|_| CliCoreError::message(format!("Invalid number for '{}': {}", key, val)))
    } else {
        Ok(default)
    }
}

/// Parse and add A records
pub(crate) fn add_a_records(
    template: &mut Template,
    records: &[String],
    global_group: &Option<String>,
    global_essential: &Option<String>,
) -> Result<(), CliCoreError> {
    for record_str in records {
        let attrs = parse_record_attrs(record_str)?;

        let host = get_optional_attr(&attrs, "host", "@");
        let value = get_required_attr(&attrs, "value", "A")?;
        let ttl = get_optional_i32(&attrs, "ttl", 3600)?;
        let group_id = attrs.get("group").cloned().or_else(|| global_group.clone());
        let essential = attrs
            .get("essential")
            .cloned()
            .or_else(|| global_essential.clone());

        // Ensure value is wrapped in %% if it's a variable name
        let points_to = if value.starts_with('%') && value.ends_with('%') {
            value
        } else {
            format!("%{}%", value)
        };

        template.records.push(Record {
            record_type: "A".to_string(),
            group_id,
            host: Some(host),
            name: None,
            points_to: Some(points_to),
            target: None,
            service: None,
            protocol: None,
            ttl: Some(ttl),
            priority: None,
            weight: None,
            port: None,
            data: None,
            essential,
            spf_rules: None,
            txt_conflict_matching_mode: None,
            txt_conflict_matching_prefix: None,
        });
    }
    Ok(())
}

/// Parse and add AAAA records
pub(crate) fn add_aaaa_records(
    template: &mut Template,
    records: &[String],
    global_group: &Option<String>,
    global_essential: &Option<String>,
) -> Result<(), CliCoreError> {
    for record_str in records {
        let attrs = parse_record_attrs(record_str)?;

        let host = get_optional_attr(&attrs, "host", "@");
        let value = get_required_attr(&attrs, "value", "AAAA")?;
        let ttl = get_optional_i32(&attrs, "ttl", 3600)?;
        let group_id = attrs.get("group").cloned().or_else(|| global_group.clone());
        let essential = attrs
            .get("essential")
            .cloned()
            .or_else(|| global_essential.clone());

        let points_to = if value.starts_with('%') && value.ends_with('%') {
            value
        } else {
            format!("%{}%", value)
        };

        template.records.push(Record {
            record_type: "AAAA".to_string(),
            group_id,
            host: Some(host),
            name: None,
            points_to: Some(points_to),
            target: None,
            service: None,
            protocol: None,
            ttl: Some(ttl),
            priority: None,
            weight: None,
            port: None,
            data: None,
            essential,
            spf_rules: None,
            txt_conflict_matching_mode: None,
            txt_conflict_matching_prefix: None,
        });
    }
    Ok(())
}

/// Parse and add CNAME records
pub(crate) fn add_cname_records(
    template: &mut Template,
    records: &[String],
    global_group: &Option<String>,
    global_essential: &Option<String>,
) -> Result<(), CliCoreError> {
    for record_str in records {
        let attrs = parse_record_attrs(record_str)?;

        let host = get_optional_attr(&attrs, "host", "@");
        let value = get_required_attr(&attrs, "value", "CNAME")?;
        let ttl = get_optional_i32(&attrs, "ttl", 3600)?;
        let group_id = attrs.get("group").cloned().or_else(|| global_group.clone());
        let essential = attrs
            .get("essential")
            .cloned()
            .or_else(|| global_essential.clone());

        template.records.push(Record {
            record_type: "CNAME".to_string(),
            group_id,
            host: Some(host),
            name: None,
            points_to: Some(value),
            target: None,
            service: None,
            protocol: None,
            ttl: Some(ttl),
            priority: None,
            weight: None,
            port: None,
            data: None,
            essential,
            spf_rules: None,
            txt_conflict_matching_mode: None,
            txt_conflict_matching_prefix: None,
        });
    }
    Ok(())
}

/// Parse and add MX records
pub(crate) fn add_mx_records(
    template: &mut Template,
    records: &[String],
    global_group: &Option<String>,
    global_essential: &Option<String>,
) -> Result<(), CliCoreError> {
    for record_str in records {
        let attrs = parse_record_attrs(record_str)?;

        let host = get_optional_attr(&attrs, "host", "@");
        let value = get_required_attr(&attrs, "value", "MX")?;
        let priority = get_optional_i32(&attrs, "priority", 10)?;
        let ttl = get_optional_i32(&attrs, "ttl", 3600)?;
        let group_id = attrs.get("group").cloned().or_else(|| global_group.clone());
        let essential = attrs
            .get("essential")
            .cloned()
            .or_else(|| global_essential.clone());

        template.records.push(Record {
            record_type: "MX".to_string(),
            group_id,
            host: Some(host),
            name: None,
            points_to: Some(value),
            target: None,
            service: None,
            protocol: None,
            ttl: Some(ttl),
            priority: Some(priority),
            weight: None,
            port: None,
            data: None,
            essential,
            spf_rules: None,
            txt_conflict_matching_mode: None,
            txt_conflict_matching_prefix: None,
        });
    }
    Ok(())
}

/// Parse and add TXT records
pub(crate) fn add_txt_records(
    template: &mut Template,
    records: &[String],
    global_group: &Option<String>,
    global_essential: &Option<String>,
) -> Result<(), CliCoreError> {
    for record_str in records {
        let attrs = parse_record_attrs(record_str)?;

        let host = get_optional_attr(&attrs, "host", "@");
        let data = get_required_attr(&attrs, "data", "TXT")?;
        let ttl = get_optional_i32(&attrs, "ttl", 3600)?;
        let group_id = attrs.get("group").cloned().or_else(|| global_group.clone());
        let essential = attrs
            .get("essential")
            .cloned()
            .or_else(|| global_essential.clone());
        let txt_conflict_matching_mode = attrs.get("mode").cloned();
        let txt_conflict_matching_prefix = attrs.get("prefix").cloned();

        template.records.push(Record {
            record_type: "TXT".to_string(),
            group_id,
            host: Some(host),
            name: None,
            points_to: None,
            target: None,
            service: None,
            protocol: None,
            ttl: Some(ttl),
            priority: None,
            weight: None,
            port: None,
            data: Some(data),
            essential,
            spf_rules: None,
            txt_conflict_matching_mode,
            txt_conflict_matching_prefix,
        });
    }
    Ok(())
}

/// Parse and add SRV records
pub(crate) fn add_srv_records(
    template: &mut Template,
    records: &[String],
    global_group: &Option<String>,
    global_essential: &Option<String>,
) -> Result<(), CliCoreError> {
    for record_str in records {
        let attrs = parse_record_attrs(record_str)?;

        let service = get_required_attr(&attrs, "service", "SRV")?;
        let protocol = get_required_attr(&attrs, "protocol", "SRV")?;
        let name = get_optional_attr(&attrs, "name", "@");
        let target = get_required_attr(&attrs, "target", "SRV")?;
        let priority = get_optional_i32(&attrs, "priority", 10)?;
        let weight = get_optional_i32(&attrs, "weight", 1)?;
        let port = get_optional_i32(&attrs, "port", 80)?;
        let ttl = get_optional_i32(&attrs, "ttl", 3600)?;
        let group_id = attrs.get("group").cloned().or_else(|| global_group.clone());
        let essential = attrs
            .get("essential")
            .cloned()
            .or_else(|| global_essential.clone());

        template.records.push(Record {
            record_type: "SRV".to_string(),
            group_id,
            host: None,
            name: Some(name),
            points_to: None,
            target: Some(target),
            service: Some(service),
            protocol: Some(protocol),
            ttl: Some(ttl),
            priority: Some(priority),
            weight: Some(weight),
            port: Some(port),
            data: None,
            essential,
            spf_rules: None,
            txt_conflict_matching_mode: None,
            txt_conflict_matching_prefix: None,
        });
    }
    Ok(())
}

/// Parse and add NS records
pub(crate) fn add_ns_records(
    template: &mut Template,
    records: &[String],
    global_group: &Option<String>,
    global_essential: &Option<String>,
) -> Result<(), CliCoreError> {
    for record_str in records {
        let attrs = parse_record_attrs(record_str)?;

        let host = get_optional_attr(&attrs, "host", "@");
        let value = get_required_attr(&attrs, "value", "NS")?;
        let ttl = get_optional_i32(&attrs, "ttl", 3600)?;
        let group_id = attrs.get("group").cloned().or_else(|| global_group.clone());
        let essential = attrs
            .get("essential")
            .cloned()
            .or_else(|| global_essential.clone());

        template.records.push(Record {
            record_type: "NS".to_string(),
            group_id,
            host: Some(host),
            name: None,
            points_to: Some(value),
            target: None,
            service: None,
            protocol: None,
            ttl: Some(ttl),
            priority: None,
            weight: None,
            port: None,
            data: None,
            essential,
            spf_rules: None,
            txt_conflict_matching_mode: None,
            txt_conflict_matching_prefix: None,
        });
    }
    Ok(())
}

/// Parse and add SPFM records
pub(crate) fn add_spfm_records(
    template: &mut Template,
    records: &[String],
    global_group: &Option<String>,
    global_essential: &Option<String>,
) -> Result<(), CliCoreError> {
    for record_str in records {
        let attrs = parse_record_attrs(record_str)?;

        let host = get_optional_attr(&attrs, "host", "@");
        let rules = get_required_attr(&attrs, "rules", "SPFM")?;
        let ttl = get_optional_i32(&attrs, "ttl", 3600)?;
        let group_id = attrs.get("group").cloned().or_else(|| global_group.clone());
        let essential = attrs
            .get("essential")
            .cloned()
            .or_else(|| global_essential.clone());

        template.records.push(Record {
            record_type: "SPFM".to_string(),
            group_id,
            host: Some(host),
            name: None,
            points_to: None,
            target: None,
            service: None,
            protocol: None,
            ttl: Some(ttl),
            priority: None,
            weight: None,
            port: None,
            data: None,
            essential,
            spf_rules: Some(rules),
            txt_conflict_matching_mode: None,
            txt_conflict_matching_prefix: None,
        });
    }
    Ok(())
}

/// Parse and add REDIR301 records
pub(crate) fn add_redir301_records(
    template: &mut Template,
    records: &[String],
    global_group: &Option<String>,
    global_essential: &Option<String>,
) -> Result<(), CliCoreError> {
    for record_str in records {
        let attrs = parse_record_attrs(record_str)?;

        let host = get_optional_attr(&attrs, "host", "@");
        let target = get_required_attr(&attrs, "target", "REDIR301")?;
        let group_id = attrs.get("group").cloned().or_else(|| global_group.clone());
        let essential = attrs
            .get("essential")
            .cloned()
            .or_else(|| global_essential.clone());

        template.records.push(Record {
            record_type: "REDIR301".to_string(),
            group_id,
            host: Some(host),
            name: None,
            points_to: None,
            target: Some(target),
            service: None,
            protocol: None,
            ttl: None,
            priority: None,
            weight: None,
            port: None,
            data: None,
            essential,
            spf_rules: None,
            txt_conflict_matching_mode: None,
            txt_conflict_matching_prefix: None,
        });
    }
    Ok(())
}

/// Parse and add REDIR302 records
pub(crate) fn add_redir302_records(
    template: &mut Template,
    records: &[String],
    global_group: &Option<String>,
    global_essential: &Option<String>,
) -> Result<(), CliCoreError> {
    for record_str in records {
        let attrs = parse_record_attrs(record_str)?;

        let host = get_optional_attr(&attrs, "host", "@");
        let target = get_required_attr(&attrs, "target", "REDIR302")?;
        let group_id = attrs.get("group").cloned().or_else(|| global_group.clone());
        let essential = attrs
            .get("essential")
            .cloned()
            .or_else(|| global_essential.clone());

        template.records.push(Record {
            record_type: "REDIR302".to_string(),
            group_id,
            host: Some(host),
            name: None,
            points_to: None,
            target: Some(target),
            service: None,
            protocol: None,
            ttl: None,
            priority: None,
            weight: None,
            port: None,
            data: None,
            essential,
            spf_rules: None,
            txt_conflict_matching_mode: None,
            txt_conflict_matching_prefix: None,
        });
    }
    Ok(())
}

/// Parse and add APEXCNAME records
pub(crate) fn add_apexcname_records(
    template: &mut Template,
    records: &[String],
    global_group: &Option<String>,
    global_essential: &Option<String>,
) -> Result<(), CliCoreError> {
    for record_str in records {
        let attrs = parse_record_attrs(record_str)?;

        let value = get_required_attr(&attrs, "value", "APEXCNAME")?;
        let group_id = attrs.get("group").cloned().or_else(|| global_group.clone());
        let essential = attrs
            .get("essential")
            .cloned()
            .or_else(|| global_essential.clone());

        template.records.push(Record {
            record_type: "APEXCNAME".to_string(),
            group_id,
            host: Some("@".to_string()),
            name: None,
            points_to: Some(value),
            target: None,
            service: None,
            protocol: None,
            ttl: None,
            priority: None,
            weight: None,
            port: None,
            data: None,
            essential,
            spf_rules: None,
            txt_conflict_matching_mode: None,
            txt_conflict_matching_prefix: None,
        });
    }
    Ok(())
}

/// Parse and add CAA records
pub(crate) fn add_caa_records(
    template: &mut Template,
    records: &[String],
    global_group: &Option<String>,
    global_essential: &Option<String>,
) -> Result<(), CliCoreError> {
    for record_str in records {
        let attrs = parse_record_attrs(record_str)?;

        let host = get_optional_attr(&attrs, "host", "@");
        let data = get_required_attr(&attrs, "data", "CAA")?;
        let ttl = get_optional_i32(&attrs, "ttl", 3600)?;
        let group_id = attrs.get("group").cloned().or_else(|| global_group.clone());
        let essential = attrs
            .get("essential")
            .cloned()
            .or_else(|| global_essential.clone());

        template.records.push(Record {
            record_type: "CAA".to_string(),
            group_id,
            host: Some(host),
            name: None,
            points_to: None,
            target: None,
            service: None,
            protocol: None,
            ttl: Some(ttl),
            priority: None,
            weight: None,
            port: None,
            data: Some(data),
            essential,
            spf_rules: None,
            txt_conflict_matching_mode: None,
            txt_conflict_matching_prefix: None,
        });
    }
    Ok(())
}
