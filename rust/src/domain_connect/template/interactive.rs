//! Interactive prompts for Domain Connect template creation

#![allow(clippy::upper_case_acronyms)]
#![allow(clippy::print_stderr)]

use super::types::Record;
use dialoguer::{Input, Select};

/// Record type enum for interactive selection
#[derive(Debug, Clone, Copy)]
pub(crate) enum RecordType {
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
    pub(crate) fn as_str(&self) -> &'static str {
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

pub(crate) fn prompt_record_type() -> Result<RecordType, cli_engine::CliCoreError> {
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

pub(crate) fn prompt_optional_string(
    prompt: &str,
) -> Result<Option<String>, cli_engine::CliCoreError> {
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

pub(crate) fn prompt_optional_i32(
    prompt: &str,
    default: i32,
) -> Result<i32, cli_engine::CliCoreError> {
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

pub(crate) fn prompt_a_record() -> Result<Record, cli_engine::CliCoreError> {
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

pub(crate) fn prompt_aaaa_record() -> Result<Record, cli_engine::CliCoreError> {
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

pub(crate) fn prompt_cname_record() -> Result<Record, cli_engine::CliCoreError> {
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

pub(crate) fn prompt_mx_record() -> Result<Record, cli_engine::CliCoreError> {
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

pub(crate) fn prompt_txt_record() -> Result<Record, cli_engine::CliCoreError> {
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

pub(crate) fn prompt_srv_record() -> Result<Record, cli_engine::CliCoreError> {
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

pub(crate) fn prompt_ns_record() -> Result<Record, cli_engine::CliCoreError> {
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

pub(crate) fn prompt_spfm_record() -> Result<Record, cli_engine::CliCoreError> {
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

pub(crate) fn prompt_redir301_record() -> Result<Record, cli_engine::CliCoreError> {
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

pub(crate) fn prompt_redir302_record() -> Result<Record, cli_engine::CliCoreError> {
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

pub(crate) fn prompt_apexcname_record() -> Result<Record, cli_engine::CliCoreError> {
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

pub(crate) fn prompt_caa_record() -> Result<Record, cli_engine::CliCoreError> {
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
