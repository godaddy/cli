//! `gddy domain-connect template init` — Create a template interactively

#![allow(clippy::print_stderr)]

use cli_engine::{CommandSpec, RuntimeCommandSpec};
use dialoguer::{Confirm, Input, Select};
use serde_json::json;
use std::collections::HashMap;

use super::{Record, Template};

#[derive(Debug, Clone, clap::Args)]
struct InitArgs {
    /// Disable interactive prompts (requires all fields as flags)
    #[arg(long = "no-interactive", default_value_t = false)]
    no_interactive: bool,

    /// Provider domain (required in non-interactive mode, e.g., acme.com)
    #[arg(long = "provider-id", required_if_eq("no_interactive", "true"))]
    provider_id: Option<String>,

    /// Provider display name (required in non-interactive mode, e.g., "Acme Corp")
    #[arg(long = "provider-name", required_if_eq("no_interactive", "true"))]
    provider_name: Option<String>,

    /// Service identifier (required in non-interactive mode, e.g., "managed-wordpress")
    #[arg(long = "service-id", required_if_eq("no_interactive", "true"))]
    service_id: Option<String>,

    /// Service display name (required in non-interactive mode, e.g., "Acme Managed WordPress")
    #[arg(long = "service-name", required_if_eq("no_interactive", "true"))]
    service_name: Option<String>,

    /// Template description
    #[arg(long = "description")]
    description: Option<String>,

    /// Block synchronous setup until complete
    #[arg(long = "sync-block")]
    sync_block: Option<bool>,

    /// Template can be shared across providers
    #[arg(long = "shared")]
    shared: Option<bool>,

    /// Provider name is shared
    #[arg(long = "shared-provider-name")]
    shared_provider_name: Option<bool>,

    /// Service name is shared
    #[arg(long = "shared-service-name")]
    shared_service_name: Option<bool>,

    /// Allow multiple instances of this template
    #[arg(long = "multi-instance")]
    multi_instance: Option<bool>,

    /// Display phishing warning to user
    #[arg(long = "warn-phishing")]
    warn_phishing: Option<bool>,

    /// Host parameter is required from user
    #[arg(long = "host-required")]
    host_required: Option<bool>,

    /// Logo URL (must be http/https with jpg/png/svg extension)
    #[arg(long = "logo-url")]
    logo_url: Option<String>,

    /// Variable description for template parameters
    #[arg(long = "variable-description")]
    variable_description: Option<String>,

    /// Sync public key domain for verification
    #[arg(long = "sync-pub-key-domain")]
    sync_pub_key_domain: Option<String>,

    /// Sync redirect domain after setup
    #[arg(long = "sync-redirect-domain")]
    sync_redirect_domain: Option<String>,

    /// Add A record (repeatable). Format: "host=@,value=ip" or "host=www,value=%IP4%,ttl=3600,group=web"
    #[arg(long = "add-a")]
    add_a: Vec<String>,

    /// Add AAAA record (repeatable). Format: "host=@,value=IP6" or "host=www,value=%ipv6%,ttl=3600"
    #[arg(long = "add-aaaa")]
    add_aaaa: Vec<String>,

    /// Add CNAME record (repeatable). Format: "host=www,value=@" or "host=blog,value=host.provider.com,ttl=3600"
    #[arg(long = "add-cname")]
    add_cname: Vec<String>,

    /// Add MX record (repeatable). Format: "value=mail.example.com,priority=10" or "host=@,value=mail.com,priority=10,ttl=3600"
    #[arg(long = "add-mx")]
    add_mx: Vec<String>,

    /// Add TXT record (repeatable). Format: "host=@,data=v=spf1 ~all" or "host=_dmarc,data=v=DMARC1...,ttl=3600"
    #[arg(long = "add-txt")]
    add_txt: Vec<String>,

    /// Add SRV record (repeatable). Format: "service=_sip,protocol=_tcp,target=sip.example.com,port=5060,priority=100,weight=1"
    #[arg(long = "add-srv")]
    add_srv: Vec<String>,

    /// Add NS record (repeatable). Format: "host=@,value=ns1.example.com" or "host=subdomain,value=ns.provider.com,ttl=3600"
    #[arg(long = "add-ns")]
    add_ns: Vec<String>,

    /// Add SPFM record (repeatable). Format: "host=@,rules=include:_spf.example.com" or "rules=include:provider.com,ttl=3600"
    #[arg(long = "add-spfm")]
    add_spfm: Vec<String>,

    /// Add REDIR301 record (repeatable). Format: "host=@,target=https://www.%fqdn%" or "host=old,target=https://new.site.com"
    #[arg(long = "add-redir301")]
    add_redir301: Vec<String>,

    /// Add REDIR302 record (repeatable). Format: "host=@,target=https://www.%fqdn%" or "host=old,target=https://new.site.com"
    #[arg(long = "add-redir302")]
    add_redir302: Vec<String>,

    /// Add APEXCNAME record (repeatable). Format: "host=@,value=target.example.com" or "value=provider.com,ttl=3600"
    #[arg(long = "add-apexcname")]
    add_apexcname: Vec<String>,

    /// Add CAA record (repeatable). Format: "host=@,data=0 issue \"letsencrypt.org\"" or "data=0 issuewild \";\",ttl=3600"
    #[arg(long = "add-caa")]
    add_caa: Vec<String>,

    /// Global group ID for all records (can be overridden per-record with group=value)
    #[arg(long = "group-id")]
    group_id: Option<String>,

    /// Global essential value for all records (can be overridden per-record with essential=value)
    #[arg(long = "essential")]
    essential: Option<String>,
}

#[allow(clippy::upper_case_acronyms)]
enum RecordType {
    A,
    AAAA,
    CNAME,
    MX,
    TXT,
    SRV,
    NS,
    SPFM,
    REDIR301,
    REDIR302,
    APEXCNAME,
    CAA,
}

impl RecordType {
    fn as_str(&self) -> &str {
        match self {
            RecordType::A => "A",
            RecordType::AAAA => "AAAA",
            RecordType::CNAME => "CNAME",
            RecordType::MX => "MX",
            RecordType::TXT => "TXT",
            RecordType::SRV => "SRV",
            RecordType::NS => "NS",
            RecordType::SPFM => "SPFM",
            RecordType::REDIR301 => "REDIR301",
            RecordType::REDIR302 => "REDIR302",
            RecordType::APEXCNAME => "APEXCNAME",
            RecordType::CAA => "CAA",
        }
    }
}

/// Parse key=value,key2=value2 format into a HashMap
fn parse_record_attrs(input: &str) -> Result<HashMap<String, String>, cli_engine::CliCoreError> {
    let mut attrs = HashMap::new();

    for pair in input.split(',') {
        let parts: Vec<&str> = pair.splitn(2, '=').collect();
        if parts.len() == 2 {
            attrs.insert(
                parts[0].trim().to_string(),
                parts[1].trim().to_string(),
            );
        } else {
            return Err(cli_engine::CliCoreError::message(format!(
                "Invalid format '{}'. Expected key=value pairs separated by commas",
                pair
            )));
        }
    }

    Ok(attrs)
}

/// Get a required attribute or return an error
fn get_required_attr(
    attrs: &HashMap<String, String>,
    key: &str,
    record_type: &str,
) -> Result<String, cli_engine::CliCoreError> {
    attrs
        .get(key)
        .cloned()
        .ok_or_else(|| {
            cli_engine::CliCoreError::message(format!(
                "{} record requires '{}' attribute",
                record_type, key
            ))
        })
}

/// Get an optional attribute with a default value
fn get_optional_attr(
    attrs: &HashMap<String, String>,
    key: &str,
    default: &str,
) -> String {
    attrs.get(key).cloned().unwrap_or_else(|| default.to_string())
}

/// Get an optional i32 attribute with a default value
fn get_optional_i32(
    attrs: &HashMap<String, String>,
    key: &str,
    default: i32,
) -> Result<i32, cli_engine::CliCoreError> {
    if let Some(val) = attrs.get(key) {
        val.parse().map_err(|_| {
            cli_engine::CliCoreError::message(format!(
                "Invalid number for '{}': {}",
                key, val
            ))
        })
    } else {
        Ok(default)
    }
}

/// Parse and add A records
fn add_a_records(
    template: &mut Template,
    records: &[String],
    global_group: &Option<String>,
    global_essential: &Option<String>,
) -> Result<(), cli_engine::CliCoreError> {
    for record_str in records {
        let attrs = parse_record_attrs(record_str)?;

        let host = get_optional_attr(&attrs, "host", "@");
        let value = get_required_attr(&attrs, "value", "A")?;
        let ttl = get_optional_i32(&attrs, "ttl", 3600)?;
        let group_id = attrs.get("group").cloned().or_else(|| global_group.clone());
        let essential = attrs.get("essential").cloned().or_else(|| global_essential.clone());

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
fn add_aaaa_records(
    template: &mut Template,
    records: &[String],
    global_group: &Option<String>,
    global_essential: &Option<String>,
) -> Result<(), cli_engine::CliCoreError> {
    for record_str in records {
        let attrs = parse_record_attrs(record_str)?;

        let host = get_optional_attr(&attrs, "host", "@");
        let value = get_required_attr(&attrs, "value", "AAAA")?;
        let ttl = get_optional_i32(&attrs, "ttl", 3600)?;
        let group_id = attrs.get("group").cloned().or_else(|| global_group.clone());
        let essential = attrs.get("essential").cloned().or_else(|| global_essential.clone());

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
fn add_cname_records(
    template: &mut Template,
    records: &[String],
    global_group: &Option<String>,
    global_essential: &Option<String>,
) -> Result<(), cli_engine::CliCoreError> {
    for record_str in records {
        let attrs = parse_record_attrs(record_str)?;

        let host = get_required_attr(&attrs, "host", "CNAME")?;
        let value = get_required_attr(&attrs, "value", "CNAME")?;
        let ttl = get_optional_i32(&attrs, "ttl", 3600)?;
        let group_id = attrs.get("group").cloned().or_else(|| global_group.clone());
        let essential = attrs.get("essential").cloned().or_else(|| global_essential.clone());

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
fn add_mx_records(
    template: &mut Template,
    records: &[String],
    global_group: &Option<String>,
    global_essential: &Option<String>,
) -> Result<(), cli_engine::CliCoreError> {
    for record_str in records {
        let attrs = parse_record_attrs(record_str)?;

        let host = get_optional_attr(&attrs, "host", "@");
        let value = get_required_attr(&attrs, "value", "MX")?;
        let priority = get_optional_i32(&attrs, "priority", 10)?;
        let ttl = get_optional_i32(&attrs, "ttl", 3600)?;
        let group_id = attrs.get("group").cloned().or_else(|| global_group.clone());
        let essential = attrs.get("essential").cloned().or_else(|| global_essential.clone());

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
fn add_txt_records(
    template: &mut Template,
    records: &[String],
    global_group: &Option<String>,
    global_essential: &Option<String>,
) -> Result<(), cli_engine::CliCoreError> {
    for record_str in records {
        let attrs = parse_record_attrs(record_str)?;

        let host = get_optional_attr(&attrs, "host", "@");
        let data = get_required_attr(&attrs, "data", "TXT")?;
        let ttl = get_optional_i32(&attrs, "ttl", 3600)?;
        let group_id = attrs.get("group").cloned().or_else(|| global_group.clone());
        let essential = attrs.get("essential").cloned().or_else(|| global_essential.clone());

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
            txt_conflict_matching_mode: None,
            txt_conflict_matching_prefix: None,
        });
    }
    Ok(())
}

/// Parse and add SRV records
fn add_srv_records(
    template: &mut Template,
    records: &[String],
    global_group: &Option<String>,
    global_essential: &Option<String>,
) -> Result<(), cli_engine::CliCoreError> {
    for record_str in records {
        let attrs = parse_record_attrs(record_str)?;

        let service = get_required_attr(&attrs, "service", "SRV")?;
        let protocol = get_required_attr(&attrs, "protocol", "SRV")?;
        let name = get_optional_attr(&attrs, "name", "@");
        let target = get_required_attr(&attrs, "target", "SRV")?;
        let port: i32 = attrs
            .get("port")
            .ok_or_else(|| cli_engine::CliCoreError::message("SRV record requires 'port' attribute"))?
            .parse()
            .map_err(|_| cli_engine::CliCoreError::message("Invalid port number"))?;
        let priority = get_optional_i32(&attrs, "priority", 100)?;
        let weight = get_optional_i32(&attrs, "weight", 1)?;
        let ttl = get_optional_i32(&attrs, "ttl", 3600)?;
        let group_id = attrs.get("group").cloned().or_else(|| global_group.clone());
        let essential = attrs.get("essential").cloned().or_else(|| global_essential.clone());

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
fn add_ns_records(
    template: &mut Template,
    records: &[String],
    global_group: &Option<String>,
    global_essential: &Option<String>,
) -> Result<(), cli_engine::CliCoreError> {
    for record_str in records {
        let attrs = parse_record_attrs(record_str)?;

        let host = get_optional_attr(&attrs, "host", "@");
        let value = get_required_attr(&attrs, "value", "NS")?;
        let ttl = get_optional_i32(&attrs, "ttl", 3600)?;
        let group_id = attrs.get("group").cloned().or_else(|| global_group.clone());
        let essential = attrs.get("essential").cloned().or_else(|| global_essential.clone());

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
fn add_spfm_records(
    template: &mut Template,
    records: &[String],
    global_group: &Option<String>,
    global_essential: &Option<String>,
) -> Result<(), cli_engine::CliCoreError> {
    for record_str in records {
        let attrs = parse_record_attrs(record_str)?;

        let host = get_optional_attr(&attrs, "host", "@");
        let rules = get_required_attr(&attrs, "rules", "SPFM")?;
        let ttl = get_optional_i32(&attrs, "ttl", 3600)?;
        let group_id = attrs.get("group").cloned().or_else(|| global_group.clone());
        let essential = attrs.get("essential").cloned().or_else(|| global_essential.clone());

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
fn add_redir301_records(
    template: &mut Template,
    records: &[String],
    global_group: &Option<String>,
    global_essential: &Option<String>,
) -> Result<(), cli_engine::CliCoreError> {
    for record_str in records {
        let attrs = parse_record_attrs(record_str)?;

        let host = get_optional_attr(&attrs, "host", "@");
        let target = get_required_attr(&attrs, "target", "REDIR301")?;
        let group_id = attrs.get("group").cloned().or_else(|| global_group.clone());
        let essential = attrs.get("essential").cloned().or_else(|| global_essential.clone());

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
fn add_redir302_records(
    template: &mut Template,
    records: &[String],
    global_group: &Option<String>,
    global_essential: &Option<String>,
) -> Result<(), cli_engine::CliCoreError> {
    for record_str in records {
        let attrs = parse_record_attrs(record_str)?;

        let host = get_optional_attr(&attrs, "host", "@");
        let target = get_required_attr(&attrs, "target", "REDIR302")?;
        let group_id = attrs.get("group").cloned().or_else(|| global_group.clone());
        let essential = attrs.get("essential").cloned().or_else(|| global_essential.clone());

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
fn add_apexcname_records(
    template: &mut Template,
    records: &[String],
    global_group: &Option<String>,
    global_essential: &Option<String>,
) -> Result<(), cli_engine::CliCoreError> {
    for record_str in records {
        let attrs = parse_record_attrs(record_str)?;

        let host = get_optional_attr(&attrs, "host", "@");
        let value = get_required_attr(&attrs, "value", "APEXCNAME")?;
        let ttl = get_optional_i32(&attrs, "ttl", 3600)?;
        let group_id = attrs.get("group").cloned().or_else(|| global_group.clone());
        let essential = attrs.get("essential").cloned().or_else(|| global_essential.clone());

        template.records.push(Record {
            record_type: "APEXCNAME".to_string(),
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

/// Parse and add CAA records
fn add_caa_records(
    template: &mut Template,
    records: &[String],
    global_group: &Option<String>,
    global_essential: &Option<String>,
) -> Result<(), cli_engine::CliCoreError> {
    for record_str in records {
        let attrs = parse_record_attrs(record_str)?;

        let host = get_optional_attr(&attrs, "host", "@");
        let data = get_required_attr(&attrs, "data", "CAA")?;
        let ttl = get_optional_i32(&attrs, "ttl", 3600)?;
        let group_id = attrs.get("group").cloned().or_else(|| global_group.clone());
        let essential = attrs.get("essential").cloned().or_else(|| global_essential.clone());

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

fn prompt_record_type() -> Result<RecordType, cli_engine::CliCoreError> {
    let options = vec![
        "A (IPv4 address)",
        "AAAA (IPv6 address)",
        "CNAME (Canonical name)",
        "MX (Mail exchanger)",
        "TXT (Text record)",
        "SRV (Service record)",
        "NS (Name server)",
        "SPFM (SPF Macro)",
        "REDIR301 (301 Permanent Redirect)",
        "REDIR302 (302 Temporary Redirect)",
        "APEXCNAME (Apex CNAME)",
        "CAA (Certificate Authority Authorization)",
    ];

    let selection = Select::new()
        .with_prompt("Record type")
        .items(&options)
        .interact()
        .map_err(|e| cli_engine::CliCoreError::message(format!("Input error: {}", e)))?;

    Ok(match selection {
        0 => RecordType::A,
        1 => RecordType::AAAA,
        2 => RecordType::CNAME,
        3 => RecordType::MX,
        4 => RecordType::TXT,
        5 => RecordType::SRV,
        6 => RecordType::NS,
        7 => RecordType::SPFM,
        8 => RecordType::REDIR301,
        9 => RecordType::REDIR302,
        10 => RecordType::APEXCNAME,
        11 => RecordType::CAA,
        _ => return Err(cli_engine::CliCoreError::message("Invalid selection")),
    })
}

fn prompt_optional_string(prompt: &str) -> Result<Option<String>, cli_engine::CliCoreError> {
    let value: String = Input::new()
        .with_prompt(prompt)
        .allow_empty(true)
        .interact_text()
        .map_err(|e| cli_engine::CliCoreError::message(format!("Input error: {}", e)))?;

    Ok(if value.trim().is_empty() {
        None
    } else {
        Some(value.trim().to_string())
    })
}

fn prompt_optional_i32(prompt: &str, default: i32) -> Result<i32, cli_engine::CliCoreError> {
    let value: String = Input::new()
        .with_prompt(format!("{} (default: {})", prompt, default))
        .allow_empty(true)
        .interact_text()
        .map_err(|e| cli_engine::CliCoreError::message(format!("Input error: {}", e)))?;

    if value.trim().is_empty() {
        Ok(default)
    } else {
        value
            .trim()
            .parse()
            .map_err(|_| cli_engine::CliCoreError::message("Must be a valid number"))
    }
}

fn prompt_a_record() -> Result<Record, cli_engine::CliCoreError> {
    eprintln!("\n━━━ A Record (IPv4) ━━━");

    let host: String = Input::new()
        .with_prompt("Host (required, e.g., @ for apex or www)")
        .interact_text()
        .map_err(|e| cli_engine::CliCoreError::message(format!("Input error: {}", e)))?;

    let ip_variable: String = Input::new()
        .with_prompt("IP variable name (required, e.g., ip, IP4, rootip)")
        .default("ip".to_string())
        .interact_text()
        .map_err(|e| cli_engine::CliCoreError::message(format!("Input error: {}", e)))?;

    let group_id = prompt_optional_string("Group ID (optional, groups related records)")?;
    let essential = prompt_optional_string("Essential (optional, OnApply/Always/No)")?;
    let ttl = prompt_optional_i32("TTL (optional)", 3600)?;

    Ok(Record {
        record_type: "A".to_string(),
        group_id,
        host: Some(host),
        name: None,
        points_to: Some(format!("%{}%", ip_variable)),
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
    })
}

fn prompt_aaaa_record() -> Result<Record, cli_engine::CliCoreError> {
    eprintln!("\n━━━ AAAA Record (IPv6) ━━━");

    let host: String = Input::new()
        .with_prompt("Host (required, e.g., @ for apex or www)")
        .interact_text()
        .map_err(|e| cli_engine::CliCoreError::message(format!("Input error: {}", e)))?;

    let ipv6_variable: String = Input::new()
        .with_prompt("IPv6 variable name (required, e.g., IP6, ipv6)")
        .default("IP6".to_string())
        .interact_text()
        .map_err(|e| cli_engine::CliCoreError::message(format!("Input error: {}", e)))?;

    let group_id = prompt_optional_string("Group ID (optional)")?;
    let essential = prompt_optional_string("Essential (optional, OnApply/Always/No)")?;
    let ttl = prompt_optional_i32("TTL (optional)", 3600)?;

    Ok(Record {
        record_type: "AAAA".to_string(),
        group_id,
        host: Some(host),
        name: None,
        points_to: Some(format!("%{}%", ipv6_variable)),
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
    })
}

fn prompt_cname_record() -> Result<Record, cli_engine::CliCoreError> {
    eprintln!("\n━━━ CNAME Record ━━━");

    let host: String = Input::new()
        .with_prompt("Host (required, e.g., www)")
        .interact_text()
        .map_err(|e| cli_engine::CliCoreError::message(format!("Input error: {}", e)))?;

    let target: String = Input::new()
        .with_prompt("Target (required, e.g., @ or example.com)")
        .interact_text()
        .map_err(|e| cli_engine::CliCoreError::message(format!("Input error: {}", e)))?;

    let group_id = prompt_optional_string("Group ID (optional)")?;
    let essential = prompt_optional_string("Essential (optional, OnApply/Always/No)")?;
    let ttl = prompt_optional_i32("TTL (optional)", 3600)?;

    Ok(Record {
        record_type: "CNAME".to_string(),
        group_id,
        host: Some(host),
        name: None,
        points_to: Some(target),
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
    })
}

fn prompt_mx_record() -> Result<Record, cli_engine::CliCoreError> {
    eprintln!("\n━━━ MX Record (Mail Exchanger) ━━━");

    let host: String = Input::new()
        .with_prompt("Host (required, e.g., @ for apex)")
        .default("@".to_string())
        .interact_text()
        .map_err(|e| cli_engine::CliCoreError::message(format!("Input error: {}", e)))?;

    let mail_server: String = Input::new()
        .with_prompt("Mail server (required, e.g., mail.example.com)")
        .interact_text()
        .map_err(|e| cli_engine::CliCoreError::message(format!("Input error: {}", e)))?;

    let priority = prompt_optional_i32("Priority (required)", 10)?;
    let group_id = prompt_optional_string("Group ID (optional, e.g., mail)")?;
    let essential = prompt_optional_string("Essential (optional, OnApply/Always/No)")?;
    let ttl = prompt_optional_i32("TTL (optional)", 3600)?;

    Ok(Record {
        record_type: "MX".to_string(),
        group_id,
        host: Some(host),
        name: None,
        points_to: Some(mail_server),
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
    })
}

fn prompt_txt_record() -> Result<Record, cli_engine::CliCoreError> {
    eprintln!("\n━━━ TXT Record ━━━");

    let host: String = Input::new()
        .with_prompt("Host (required, e.g., @ or _dmarc)")
        .interact_text()
        .map_err(|e| cli_engine::CliCoreError::message(format!("Input error: {}", e)))?;

    let data: String = Input::new()
        .with_prompt("Data (required, e.g., v=spf1 ~all)")
        .interact_text()
        .map_err(|e| cli_engine::CliCoreError::message(format!("Input error: {}", e)))?;

    let group_id = prompt_optional_string("Group ID (optional)")?;
    let essential = prompt_optional_string("Essential (optional, OnApply/Always/No)")?;
    let ttl = prompt_optional_i32("TTL (optional)", 3600)?;

    Ok(Record {
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
            txt_conflict_matching_mode: None,
            txt_conflict_matching_prefix: None,
    })
}

fn prompt_srv_record() -> Result<Record, cli_engine::CliCoreError> {
    eprintln!("\n━━━ SRV Record (Service Locator) ━━━");
    eprintln!("    Example: _sip._tcp points to sip.example.com:5060");

    let service: String = Input::new()
        .with_prompt("Service (required, e.g., _sip)")
        .interact_text()
        .map_err(|e| cli_engine::CliCoreError::message(format!("Input error: {}", e)))?;

    let protocol: String = Input::new()
        .with_prompt("Protocol (required, e.g., _tcp, _udp)")
        .interact_text()
        .map_err(|e| cli_engine::CliCoreError::message(format!("Input error: {}", e)))?;

    let name: String = Input::new()
        .with_prompt("Name/Host (required, e.g., @)")
        .default("@".to_string())
        .interact_text()
        .map_err(|e| cli_engine::CliCoreError::message(format!("Input error: {}", e)))?;

    let target: String = Input::new()
        .with_prompt("Target (required, e.g., sip.example.com)")
        .interact_text()
        .map_err(|e| cli_engine::CliCoreError::message(format!("Input error: {}", e)))?;

    let priority = prompt_optional_i32("Priority (required)", 100)?;
    let weight = prompt_optional_i32("Weight (required)", 1)?;

    let port: i32 = Input::new()
        .with_prompt("Port (required, e.g., 5060)")
        .interact_text()
        .map_err(|e| cli_engine::CliCoreError::message(format!("Input error: {}", e)))?;

    let group_id = prompt_optional_string("Group ID (optional)")?;
    let essential = prompt_optional_string("Essential (optional, OnApply/Always/No)")?;
    let ttl = prompt_optional_i32("TTL (optional)", 3600)?;

    Ok(Record {
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
    })
}

fn prompt_ns_record() -> Result<Record, cli_engine::CliCoreError> {
    eprintln!("\n━━━ NS Record (Name Server) ━━━");

    let host: String = Input::new()
        .with_prompt("Host (required, e.g., @ or subdomain)")
        .interact_text()
        .map_err(|e| cli_engine::CliCoreError::message(format!("Input error: {}", e)))?;

    let nameserver: String = Input::new()
        .with_prompt("Name server (required, e.g., ns1.example.com)")
        .interact_text()
        .map_err(|e| cli_engine::CliCoreError::message(format!("Input error: {}", e)))?;

    let group_id = prompt_optional_string("Group ID (optional)")?;
    let essential = prompt_optional_string("Essential (optional, OnApply/Always/No)")?;
    let ttl = prompt_optional_i32("TTL (optional)", 3600)?;

    Ok(Record {
        record_type: "NS".to_string(),
        group_id,
        host: Some(host),
        name: None,
        points_to: Some(nameserver),
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
    })
}

fn prompt_spfm_record() -> Result<Record, cli_engine::CliCoreError> {
    eprintln!("\n━━━ SPFM Record (SPF Macro) ━━━");
    eprintln!("    Domain Connect specific - SPF include rules");

    let host: String = Input::new()
        .with_prompt("Host (required, e.g., @)")
        .default("@".to_string())
        .interact_text()
        .map_err(|e| cli_engine::CliCoreError::message(format!("Input error: {}", e)))?;

    let spf_rules: String = Input::new()
        .with_prompt("SPF rules (required, e.g., include:_spf.example.com)")
        .interact_text()
        .map_err(|e| cli_engine::CliCoreError::message(format!("Input error: {}", e)))?;

    let group_id = prompt_optional_string("Group ID (optional)")?;
    let essential = prompt_optional_string("Essential (optional, OnApply/Always/No)")?;
    let ttl = prompt_optional_i32("TTL (optional)", 3600)?;

    Ok(Record {
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
        spf_rules: Some(spf_rules),
            txt_conflict_matching_mode: None,
            txt_conflict_matching_prefix: None,
    })
}

fn prompt_redir301_record() -> Result<Record, cli_engine::CliCoreError> {
    eprintln!("\n━━━ REDIR301 Record (301 Redirect) ━━━");
    eprintln!("    Domain Connect specific - HTTP 301 redirect");

    let host: String = Input::new()
        .with_prompt("Host (required, e.g., @ or www)")
        .interact_text()
        .map_err(|e| cli_engine::CliCoreError::message(format!("Input error: {}", e)))?;

    let target: String = Input::new()
        .with_prompt("Redirect target (required, e.g., https://www.%fqdn%)")
        .interact_text()
        .map_err(|e| cli_engine::CliCoreError::message(format!("Input error: {}", e)))?;

    let group_id = prompt_optional_string("Group ID (optional)")?;
    let essential = prompt_optional_string("Essential (optional, OnApply/Always/No)")?;

    Ok(Record {
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
    })
}

fn prompt_redir302_record() -> Result<Record, cli_engine::CliCoreError> {
    eprintln!("\n━━━ REDIR302 Record (302 Redirect) ━━━");
    eprintln!("    Domain Connect specific - HTTP 302 temporary redirect");

    let host: String = Input::new()
        .with_prompt("Host (required, e.g., @ or www)")
        .interact_text()
        .map_err(|e| cli_engine::CliCoreError::message(format!("Input error: {}", e)))?;

    let target: String = Input::new()
        .with_prompt("Redirect target (required, e.g., https://www.%fqdn%)")
        .interact_text()
        .map_err(|e| cli_engine::CliCoreError::message(format!("Input error: {}", e)))?;

    let group_id = prompt_optional_string("Group ID (optional)")?;
    let essential = prompt_optional_string("Essential (optional, OnApply/Always/No)")?;

    Ok(Record {
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
    })
}

fn prompt_apexcname_record() -> Result<Record, cli_engine::CliCoreError> {
    eprintln!("\n━━━ APEXCNAME Record (Apex CNAME) ━━━");
    eprintln!("    Domain Connect specific - CNAME at domain apex");

    let host: String = Input::new()
        .with_prompt("Host (required, typically @)")
        .default("@".to_string())
        .interact_text()
        .map_err(|e| cli_engine::CliCoreError::message(format!("Input error: {}", e)))?;

    let points_to: String = Input::new()
        .with_prompt("Points to (required, e.g., target.example.com)")
        .interact_text()
        .map_err(|e| cli_engine::CliCoreError::message(format!("Input error: {}", e)))?;

    let group_id = prompt_optional_string("Group ID (optional)")?;
    let essential = prompt_optional_string("Essential (optional, OnApply/Always/No)")?;
    let ttl = prompt_optional_i32("TTL (optional)", 3600)?;

    Ok(Record {
        record_type: "APEXCNAME".to_string(),
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
    })
}

fn prompt_caa_record() -> Result<Record, cli_engine::CliCoreError> {
    eprintln!("\n━━━ CAA Record (Certificate Authority Authorization) ━━━");

    let host: String = Input::new()
        .with_prompt("Host (required, e.g., @)")
        .default("@".to_string())
        .interact_text()
        .map_err(|e| cli_engine::CliCoreError::message(format!("Input error: {}", e)))?;

    let data: String = Input::new()
        .with_prompt("Data (required, e.g., 0 issue \"letsencrypt.org\")")
        .interact_text()
        .map_err(|e| cli_engine::CliCoreError::message(format!("Input error: {}", e)))?;

    let group_id = prompt_optional_string("Group ID (optional)")?;
    let essential = prompt_optional_string("Essential (optional, OnApply/Always/No)")?;
    let ttl = prompt_optional_i32("TTL (optional)", 3600)?;

    Ok(Record {
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
    })
}

fn run_interactive() -> Result<Template, cli_engine::CliCoreError> {
    eprintln!("\n📝 Create Domain Connect Template\n");

    // Step 1: Basic template info
    let provider_id: String = Input::new()
        .with_prompt("Provider ID (e.g., acme.com)")
        .validate_with(|input: &String| {
            if input.contains('.') {
                Ok(())
            } else {
                Err("Provider ID must be a domain (e.g., acme.com)")
            }
        })
        .interact_text()
        .map_err(|e| cli_engine::CliCoreError::message(format!("Input error: {}", e)))?;

    let provider_name: String = Input::new()
        .with_prompt("Provider Name")
        .interact_text()
        .map_err(|e| cli_engine::CliCoreError::message(format!("Input error: {}", e)))?;

    let service_id: String = Input::new()
        .with_prompt("Service ID (e.g., email)")
        .validate_with(|input: &String| {
            if input.chars().all(|c| c.is_alphanumeric() || c == '-') {
                Ok(())
            } else {
                Err("Service ID must be alphanumeric with hyphens only")
            }
        })
        .interact_text()
        .map_err(|e| cli_engine::CliCoreError::message(format!("Input error: {}", e)))?;

    let service_name: String = Input::new()
        .with_prompt("Service Name")
        .interact_text()
        .map_err(|e| cli_engine::CliCoreError::message(format!("Input error: {}", e)))?;

    let description = prompt_optional_string("Description (optional)")?;

    let mut template = Template::from_basic_inputs(
        provider_id,
        provider_name,
        service_id,
        service_name,
        description,
    );

    // Step 2: Add records
    eprintln!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    eprintln!("📋 Add DNS Records\n");

    loop {
        let add_record = Confirm::new()
            .with_prompt("Add a record?")
            .default(false)
            .interact()
            .map_err(|e| cli_engine::CliCoreError::message(format!("Input error: {}", e)))?;

        if !add_record {
            break;
        }

        let record_type = prompt_record_type()?;

        let record = match record_type {
            RecordType::A => prompt_a_record()?,
            RecordType::AAAA => prompt_aaaa_record()?,
            RecordType::CNAME => prompt_cname_record()?,
            RecordType::MX => prompt_mx_record()?,
            RecordType::TXT => prompt_txt_record()?,
            RecordType::SRV => prompt_srv_record()?,
            RecordType::NS => prompt_ns_record()?,
            RecordType::SPFM => prompt_spfm_record()?,
            RecordType::REDIR301 => prompt_redir301_record()?,
            RecordType::REDIR302 => prompt_redir302_record()?,
            RecordType::APEXCNAME => prompt_apexcname_record()?,
            RecordType::CAA => prompt_caa_record()?,
        };

        template.records.push(record);
        eprintln!("\n✅ Added {} record\n", record_type.as_str());
    }

    Ok(template)
}

pub fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<InitArgs, _, _, _>(
        CommandSpec::from_args::<InitArgs>("init", "Create a new Domain Connect template")
            .with_long(
                "Create a Domain Connect template.\n\
                 \n\
                 By default, runs in interactive mode with prompts.\n\
                 Use --no-interactive with flags for scripting.\n\
                 \n\
                 Interactive example:\n\
                   gddy domain-connect template init\n\
                 \n\
                 Non-interactive example (multiple records with key=value format):\n\
                   gddy domain-connect template init \\\n\
                     --no-interactive \\\n\
                     --provider-id acme.com \\\n\
                     --provider-name \"Acme Corp\" \\\n\
                     --service-id email \\\n\
                     --service-name \"Email Service\" \\\n\
                     --description \"Email configuration\" \\\n\
                     --add-mx \"value=mail.acme.com,priority=10\" \\\n\
                     --add-mx \"value=mail2.acme.com,priority=20\" \\\n\
                     --add-txt \"data=v=spf1 include:_spf.acme.com ~all\" \\\n\
                     --add-cname \"host=dkim._domainkey,value=dkim.acme.com\"\n\
                 \n\
                 Record format: key=value pairs separated by commas.\n\
                 Common keys: host, value, data, priority, ttl, group, essential\n\
                 Example: --add-mx \"host=@,value=mail.com,priority=10,ttl=3600\"",
            ),
        |_ctx, args: InitArgs| async move {
            let template = if args.no_interactive {
                // Non-interactive mode: build from flags
                let provider_id = args
                    .provider_id
                    .ok_or_else(|| cli_engine::CliCoreError::message("--provider-id required"))?;
                let provider_name = args
                    .provider_name
                    .ok_or_else(|| cli_engine::CliCoreError::message("--provider-name required"))?;
                let service_id = args
                    .service_id
                    .ok_or_else(|| cli_engine::CliCoreError::message("--service-id required"))?;
                let service_name = args
                    .service_name
                    .ok_or_else(|| cli_engine::CliCoreError::message("--service-name required"))?;

                // Validate provider ID (should be domain-like)
                if !provider_id.contains('.') {
                    return Err(cli_engine::CliCoreError::message(
                        "Provider ID must be a domain (e.g., acme.com)",
                    ));
                }

                // Validate service ID (alphanumeric + hyphen)
                if !service_id.chars().all(|c| c.is_alphanumeric() || c == '-') {
                    return Err(cli_engine::CliCoreError::message(
                        "Service ID must be alphanumeric with hyphens only",
                    ));
                }

                // Build template from flags
                let mut template = Template::from_basic_inputs(
                    provider_id.clone(),
                    provider_name.clone(),
                    service_id.clone(),
                    service_name.clone(),
                    args.description.clone(),
                );

                // Set additional template fields from args
                template.logo_url = args.logo_url.clone();
                template.variable_description = args.variable_description.clone();
                template.sync_pub_key_domain = args.sync_pub_key_domain.clone();
                template.sync_redirect_domain = args.sync_redirect_domain.clone();
                template.sync_block = args.sync_block;
                template.shared = args.shared;
                template.shared_provider_name = args.shared_provider_name;
                template.shared_service_name = args.shared_service_name;
                template.multi_instance = args.multi_instance;
                template.warn_phishing = args.warn_phishing;
                template.host_required = args.host_required;

                // Add records from flags using key=value parsers
                add_a_records(&mut template, &args.add_a, &args.group_id, &args.essential)?;
                add_aaaa_records(&mut template, &args.add_aaaa, &args.group_id, &args.essential)?;
                add_cname_records(&mut template, &args.add_cname, &args.group_id, &args.essential)?;
                add_mx_records(&mut template, &args.add_mx, &args.group_id, &args.essential)?;
                add_txt_records(&mut template, &args.add_txt, &args.group_id, &args.essential)?;
                add_srv_records(&mut template, &args.add_srv, &args.group_id, &args.essential)?;
                add_ns_records(&mut template, &args.add_ns, &args.group_id, &args.essential)?;
                add_spfm_records(&mut template, &args.add_spfm, &args.group_id, &args.essential)?;
                add_redir301_records(&mut template, &args.add_redir301, &args.group_id, &args.essential)?;
                add_redir302_records(&mut template, &args.add_redir302, &args.group_id, &args.essential)?;
                add_apexcname_records(&mut template, &args.add_apexcname, &args.group_id, &args.essential)?;
                add_caa_records(&mut template, &args.add_caa, &args.group_id, &args.essential)?;

                template
            } else {
                // Interactive mode
                run_interactive()?
            };

            // Convert to JSON
            let json_output = template
                .to_json_pretty()
                .map_err(|e| cli_engine::CliCoreError::message(e.to_string()))?;

            // Generate filename from template
            let filename = format!("{}.{}.json", template.provider_id, template.service_id);

            // Get full path for display
            let full_path = std::env::current_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from("."))
                .join(&filename);

            // Write to file
            std::fs::write(&filename, &json_output).map_err(|e| {
                cli_engine::CliCoreError::message(format!(
                    "Failed to write file {}: {}",
                    filename, e
                ))
            })?;

            // Print to stderr so user can see it
            eprintln!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
            eprintln!("📄 Generated Domain Connect Template:\n");
            eprintln!("{}", json_output);
            eprintln!("\n✅ Template saved to: {}", full_path.display());
            eprintln!(
                "   {} record{}",
                template.records.len(),
                if template.records.len() == 1 {
                    ""
                } else {
                    "s"
                }
            );

            eprintln!(
                "\n💡 Next steps:\n   1. Review: cat {}\n   2. Submit: gddy domain-connect template submit {}",
                filename, filename
            );

            // Return structured result
            Ok(cli_engine::CommandResult::new(json!({
                "providerId": template.provider_id,
                "serviceId": template.service_id,
                "fileName": filename,
                "recordCount": template.records.len(),
                "template": template,
            })))
        },
    )
}
