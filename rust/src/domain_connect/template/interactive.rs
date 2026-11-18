//! Interactive prompts for Domain Connect template creation

#![allow(clippy::upper_case_acronyms)]

use std::io::{self, BufRead, Write};

use super::types::Record;
use crate::error::GddyError;

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

pub(crate) fn prompt_record_type<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
) -> cli_engine::Result<RecordType> {
    let options = vec![
        ("A", "IPv4 address"),
        ("AAAA", "IPv6 address"),
        ("CNAME", "Canonical name"),
        ("MX", "Mail exchanger"),
        ("TXT", "Text record"),
        ("SRV", "Service record"),
        ("NS", "Name server"),
        ("SPFM", "SPF Macro"),
        ("REDIR301", "301 Permanent Redirect"),
        ("REDIR302", "302 Temporary Redirect"),
        ("APEXCNAME", "Apex CNAME"),
        ("CAA", "Certificate Authority Authorization"),
    ];

    writeln!(writer, "\nRecord type:")?;
    for (idx, (record_type, description)) in options.iter().enumerate() {
        writeln!(writer, "  {}. {} ({})", idx + 1, record_type, description)?;
    }
    write!(writer, "Select (1-{}): ", options.len())?;
    writer.flush()?;

    let mut line = String::new();
    reader.read_line(&mut line)?;

    let selection = line.trim().parse::<usize>().map_err(|_| {
        GddyError::validation("Must be a valid number")
            .with_fix("Enter a number from 1 to 12")
            .into_cli_error()
    })?;

    match selection {
        1 => Ok(RecordType::A),
        2 => Ok(RecordType::AAAA),
        3 => Ok(RecordType::CNAME),
        4 => Ok(RecordType::MX),
        5 => Ok(RecordType::TXT),
        6 => Ok(RecordType::SRV),
        7 => Ok(RecordType::NS),
        8 => Ok(RecordType::SPFM),
        9 => Ok(RecordType::REDIR301),
        10 => Ok(RecordType::REDIR302),
        11 => Ok(RecordType::APEXCNAME),
        12 => Ok(RecordType::CAA),
        _ => Err(GddyError::validation(format!(
            "Invalid selection: {}. Must be between 1 and 12",
            selection
        ))
        .with_fix("Enter a number from 1 to 12")
        .into_cli_error()),
    }
}

pub(crate) fn prompt_optional_string<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
    prompt: &str,
) -> io::Result<Option<String>> {
    write!(writer, "{}: ", prompt)?;
    writer.flush()?;

    let mut line = String::new();
    reader.read_line(&mut line)?;

    let trimmed = line.trim();
    Ok(if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    })
}

pub(crate) fn prompt_string<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
    prompt: &str,
) -> cli_engine::Result<String> {
    write!(writer, "{}: ", prompt)?;
    writer.flush()?;

    let mut line = String::new();
    reader.read_line(&mut line)?;

    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Err(GddyError::validation(format!("{} cannot be empty", prompt))
            .with_fix("Enter a non-empty value")
            .into_cli_error());
    }

    Ok(trimmed.to_string())
}

pub(crate) fn prompt_string_with_default<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
    prompt: &str,
    default: &str,
) -> io::Result<String> {
    write!(writer, "{} (default: {}): ", prompt, default)?;
    writer.flush()?;

    let mut line = String::new();
    reader.read_line(&mut line)?;

    let trimmed = line.trim();
    Ok(if trimmed.is_empty() {
        default.to_string()
    } else {
        trimmed.to_string()
    })
}

pub(crate) fn prompt_optional_i32<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
    prompt: &str,
    default: i32,
) -> cli_engine::Result<i32> {
    write!(writer, "{} (default: {}): ", prompt, default)?;
    writer.flush()?;

    let mut line = String::new();
    reader.read_line(&mut line)?;

    let trimmed = line.trim();
    if trimmed.is_empty() {
        Ok(default)
    } else {
        trimmed.parse().map_err(|_| {
            GddyError::validation("Must be a valid number")
                .with_fix("Enter an integer value")
                .into_cli_error()
        })
    }
}

pub(crate) fn prompt_i32<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
    prompt: &str,
) -> cli_engine::Result<i32> {
    write!(writer, "{}: ", prompt)?;
    writer.flush()?;

    let mut line = String::new();
    reader.read_line(&mut line)?;

    line.trim().parse().map_err(|_| {
        GddyError::validation("Must be a valid number")
            .with_fix("Enter an integer value")
            .into_cli_error()
    })
}

pub(crate) fn prompt_a_record<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
) -> cli_engine::Result<Record> {
    writeln!(writer, "\n━━━ A Record (IPv4) ━━━")?;

    let host = prompt_string(reader, writer, "Host (required, e.g., @ for apex or www)")?;
    let ip_variable = prompt_string_with_default(
        reader,
        writer,
        "IP variable name (required, e.g., ip, IP4, rootip)",
        "ip",
    )?;

    let group_id = prompt_optional_string(
        reader,
        writer,
        "Group ID (optional, groups related records)",
    )?;
    let essential =
        prompt_optional_string(reader, writer, "Essential (optional, OnApply/Always/No)")?;
    let ttl = prompt_optional_i32(reader, writer, "TTL (optional)", 3600)?;

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

pub(crate) fn prompt_aaaa_record<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
) -> cli_engine::Result<Record> {
    writeln!(writer, "\n━━━ AAAA Record (IPv6) ━━━")?;

    let host = prompt_string(reader, writer, "Host (required, e.g., @ for apex or www)")?;
    let ipv6_variable = prompt_string_with_default(
        reader,
        writer,
        "IPv6 variable name (required, e.g., IP6, ipv6)",
        "IP6",
    )?;

    let group_id = prompt_optional_string(reader, writer, "Group ID (optional)")?;
    let essential =
        prompt_optional_string(reader, writer, "Essential (optional, OnApply/Always/No)")?;
    let ttl = prompt_optional_i32(reader, writer, "TTL (optional)", 3600)?;

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

pub(crate) fn prompt_cname_record<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
) -> cli_engine::Result<Record> {
    writeln!(writer, "\n━━━ CNAME Record ━━━")?;

    let host = prompt_string(reader, writer, "Host (required, e.g., www)")?;
    let target = prompt_string(reader, writer, "Target (required, e.g., @ or example.com)")?;

    let group_id = prompt_optional_string(reader, writer, "Group ID (optional)")?;
    let essential =
        prompt_optional_string(reader, writer, "Essential (optional, OnApply/Always/No)")?;
    let ttl = prompt_optional_i32(reader, writer, "TTL (optional)", 3600)?;

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

pub(crate) fn prompt_mx_record<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
) -> cli_engine::Result<Record> {
    writeln!(writer, "\n━━━ MX Record (Mail Exchanger) ━━━")?;

    let host =
        prompt_string_with_default(reader, writer, "Host (required, e.g., @ for apex)", "@")?;
    let mail_server = prompt_string(
        reader,
        writer,
        "Mail server (required, e.g., mail.example.com)",
    )?;

    let priority = prompt_optional_i32(reader, writer, "Priority (required)", 10)?;
    let group_id = prompt_optional_string(reader, writer, "Group ID (optional, e.g., mail)")?;
    let essential =
        prompt_optional_string(reader, writer, "Essential (optional, OnApply/Always/No)")?;
    let ttl = prompt_optional_i32(reader, writer, "TTL (optional)", 3600)?;

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

pub(crate) fn prompt_txt_record<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
) -> cli_engine::Result<Record> {
    writeln!(writer, "\n━━━ TXT Record ━━━")?;

    let host = prompt_string(reader, writer, "Host (required, e.g., @ or _dmarc)")?;
    let data = prompt_string(reader, writer, "Data (required, e.g., v=spf1 ~all)")?;

    let group_id = prompt_optional_string(reader, writer, "Group ID (optional)")?;
    let essential =
        prompt_optional_string(reader, writer, "Essential (optional, OnApply/Always/No)")?;
    let ttl = prompt_optional_i32(reader, writer, "TTL (optional)", 3600)?;

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

pub(crate) fn prompt_srv_record<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
) -> cli_engine::Result<Record> {
    writeln!(writer, "\n━━━ SRV Record (Service Locator) ━━━")?;
    writeln!(
        writer,
        "    Example: _sip._tcp points to sip.example.com:5060"
    )?;

    let service = prompt_string(reader, writer, "Service (required, e.g., _sip)")?;
    let protocol = prompt_string(reader, writer, "Protocol (required, e.g., _tcp, _udp)")?;
    let name = prompt_string_with_default(reader, writer, "Name/Host (required, e.g., @)", "@")?;
    let target = prompt_string(reader, writer, "Target (required, e.g., sip.example.com)")?;

    let priority = prompt_optional_i32(reader, writer, "Priority (required)", 100)?;
    let weight = prompt_optional_i32(reader, writer, "Weight (required)", 1)?;
    let port = prompt_i32(reader, writer, "Port (required, e.g., 5060)")?;

    let group_id = prompt_optional_string(reader, writer, "Group ID (optional)")?;
    let essential =
        prompt_optional_string(reader, writer, "Essential (optional, OnApply/Always/No)")?;
    let ttl = prompt_optional_i32(reader, writer, "TTL (optional)", 3600)?;

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

pub(crate) fn prompt_ns_record<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
) -> cli_engine::Result<Record> {
    writeln!(writer, "\n━━━ NS Record (Name Server) ━━━")?;

    let host = prompt_string(reader, writer, "Host (required, e.g., @ or subdomain)")?;
    let nameserver = prompt_string(
        reader,
        writer,
        "Name server (required, e.g., ns1.example.com)",
    )?;

    let group_id = prompt_optional_string(reader, writer, "Group ID (optional)")?;
    let essential =
        prompt_optional_string(reader, writer, "Essential (optional, OnApply/Always/No)")?;
    let ttl = prompt_optional_i32(reader, writer, "TTL (optional)", 3600)?;

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

pub(crate) fn prompt_spfm_record<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
) -> cli_engine::Result<Record> {
    writeln!(writer, "\n━━━ SPFM Record (SPF Macro) ━━━")?;
    writeln!(writer, "    Domain Connect specific - SPF include rules")?;

    let host = prompt_string_with_default(reader, writer, "Host (required, e.g., @)", "@")?;
    let spf_rules = prompt_string(
        reader,
        writer,
        "SPF rules (required, e.g., include:_spf.example.com)",
    )?;

    let group_id = prompt_optional_string(reader, writer, "Group ID (optional)")?;
    let essential =
        prompt_optional_string(reader, writer, "Essential (optional, OnApply/Always/No)")?;
    let ttl = prompt_optional_i32(reader, writer, "TTL (optional)", 3600)?;

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

pub(crate) fn prompt_redir301_record<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
) -> cli_engine::Result<Record> {
    writeln!(writer, "\n━━━ REDIR301 Record (301 Redirect) ━━━")?;
    writeln!(writer, "    Domain Connect specific - HTTP 301 redirect")?;

    let host = prompt_string(reader, writer, "Host (required, e.g., @ or www)")?;
    let target = prompt_string(
        reader,
        writer,
        "Redirect target (required, e.g., https://www.%fqdn%)",
    )?;

    let group_id = prompt_optional_string(reader, writer, "Group ID (optional)")?;
    let essential =
        prompt_optional_string(reader, writer, "Essential (optional, OnApply/Always/No)")?;

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

pub(crate) fn prompt_redir302_record<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
) -> cli_engine::Result<Record> {
    writeln!(writer, "\n━━━ REDIR302 Record (302 Redirect) ━━━")?;
    writeln!(
        writer,
        "    Domain Connect specific - HTTP 302 temporary redirect"
    )?;

    let host = prompt_string(reader, writer, "Host (required, e.g., @ or www)")?;
    let target = prompt_string(
        reader,
        writer,
        "Redirect target (required, e.g., https://www.%fqdn%)",
    )?;

    let group_id = prompt_optional_string(reader, writer, "Group ID (optional)")?;
    let essential =
        prompt_optional_string(reader, writer, "Essential (optional, OnApply/Always/No)")?;

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

pub(crate) fn prompt_apexcname_record<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
) -> cli_engine::Result<Record> {
    writeln!(writer, "\n━━━ APEXCNAME Record (Apex CNAME) ━━━")?;
    writeln!(writer, "    Domain Connect specific - CNAME at domain apex")?;

    let host = prompt_string_with_default(reader, writer, "Host (required, typically @)", "@")?;
    let points_to = prompt_string(
        reader,
        writer,
        "Points to (required, e.g., target.example.com)",
    )?;

    let group_id = prompt_optional_string(reader, writer, "Group ID (optional)")?;
    let essential =
        prompt_optional_string(reader, writer, "Essential (optional, OnApply/Always/No)")?;
    let ttl = prompt_optional_i32(reader, writer, "TTL (optional)", 3600)?;

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

pub(crate) fn prompt_caa_record<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
) -> cli_engine::Result<Record> {
    writeln!(
        writer,
        "\n━━━ CAA Record (Certificate Authority Authorization) ━━━"
    )?;

    let host = prompt_string_with_default(reader, writer, "Host (required, e.g., @)", "@")?;
    let data = prompt_string(
        reader,
        writer,
        "Data (required, e.g., 0 issue \"letsencrypt.org\")",
    )?;

    let group_id = prompt_optional_string(reader, writer, "Group ID (optional)")?;
    let essential =
        prompt_optional_string(reader, writer, "Essential (optional, OnApply/Always/No)")?;
    let ttl = prompt_optional_i32(reader, writer, "TTL (optional)", 3600)?;

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

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    #[test]
    fn prompt_string_accepts_input() {
        let mut input = Cursor::new(b"acme.com\n");
        let mut output = Vec::new();
        let result = prompt_string(&mut input, &mut output, "Provider ID");
        assert_eq!(result.expect("prompt"), "acme.com");
    }

    #[test]
    fn prompt_string_rejects_empty() {
        let mut input = Cursor::new(b"\n");
        let mut output = Vec::new();
        let result = prompt_string(&mut input, &mut output, "Provider ID");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("cannot be empty"));
    }

    #[test]
    fn prompt_optional_string_allows_empty() {
        let mut input = Cursor::new(b"\n");
        let mut output = Vec::new();
        let result = prompt_optional_string(&mut input, &mut output, "Description");
        assert_eq!(result.expect("prompt"), None);
    }

    #[test]
    fn prompt_optional_string_returns_value() {
        let mut input = Cursor::new(b"test value\n");
        let mut output = Vec::new();
        let result = prompt_optional_string(&mut input, &mut output, "Description");
        assert_eq!(result.expect("prompt"), Some("test value".to_string()));
    }

    #[test]
    fn prompt_string_with_default_uses_default_on_empty() {
        let mut input = Cursor::new(b"\n");
        let mut output = Vec::new();
        let result = prompt_string_with_default(&mut input, &mut output, "Host", "@");
        assert_eq!(result.expect("prompt"), "@");
    }

    #[test]
    fn prompt_string_with_default_accepts_value() {
        let mut input = Cursor::new(b"www\n");
        let mut output = Vec::new();
        let result = prompt_string_with_default(&mut input, &mut output, "Host", "@");
        assert_eq!(result.expect("prompt"), "www");
    }

    #[test]
    fn prompt_optional_i32_uses_default_on_empty() {
        let mut input = Cursor::new(b"\n");
        let mut output = Vec::new();
        let result = prompt_optional_i32(&mut input, &mut output, "TTL", 3600);
        assert_eq!(result.expect("prompt"), 3600);
    }

    #[test]
    fn prompt_optional_i32_accepts_value() {
        let mut input = Cursor::new(b"7200\n");
        let mut output = Vec::new();
        let result = prompt_optional_i32(&mut input, &mut output, "TTL", 3600);
        assert_eq!(result.expect("prompt"), 7200);
    }

    #[test]
    fn prompt_optional_i32_rejects_invalid() {
        let mut input = Cursor::new(b"not-a-number\n");
        let mut output = Vec::new();
        let result = prompt_optional_i32(&mut input, &mut output, "TTL", 3600);
        assert!(result.is_err());
    }

    #[test]
    fn prompt_record_type_accepts_valid_selection() {
        let mut input = Cursor::new(b"1\n");
        let mut output = Vec::new();
        let result = prompt_record_type(&mut input, &mut output);
        assert!(matches!(result.expect("prompt"), RecordType::A));
    }

    #[test]
    fn prompt_record_type_rejects_out_of_range() {
        let mut input = Cursor::new(b"99\n");
        let mut output = Vec::new();
        let result = prompt_record_type(&mut input, &mut output);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Invalid selection")
        );
    }
}
