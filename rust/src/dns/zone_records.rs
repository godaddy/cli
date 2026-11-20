//! Converting between the API's DNS records and the records of a zone file or
//! of `dns list -o json`, for `dns export` and `dns import`.

use std::net::{Ipv4Addr, Ipv6Addr};

use domains_client::types;
use rfc1035_format::{Name, ParseOptions, RData, Record, Svcb, Zone};
use serde_json::Value;

use super::records::{DEFAULT_TTL, RecordOptions, composed_owner, record_value, v3_record};

/// One record to write, in the pieces `dns add` and `dns set` build records from.
#[derive(Debug, Clone)]
pub(super) struct Desired {
    /// Where it came from, for messages: `line 7` or `record 3`.
    pub(super) at: String,
    pub(super) record_type: String,
    /// The record's own name: `@`, `www`, or for SRV and TLSA the name after the
    /// service and protocol.
    pub(super) name: String,
    /// The value `--data` would carry; TLSA's certificate data.
    pub(super) value: String,
    pub(super) opts: RecordOptions,
}

impl Desired {
    pub(super) fn record(&self) -> types::DnsRecord {
        v3_record(&self.name, &self.record_type, &self.value, &self.opts)
    }

    /// The name the record is filed under, lower-case (see [`composed_owner`]).
    pub(super) fn owner(&self) -> String {
        composed_owner(&self.record())
            .unwrap_or_else(|| self.name.clone())
            .to_ascii_lowercase()
    }

    pub(super) fn ttl(&self) -> i64 {
        self.opts.ttl.unwrap_or(DEFAULT_TTL)
    }

    /// `A www 192.0.2.1`, for messages.
    pub(super) fn label(&self) -> String {
        format!("{} {} {}", self.record_type, self.name, self.value)
    }
}

/// A record left out of an import because GoDaddy manages it.
#[derive(Debug, Clone)]
pub(super) struct Skipped {
    pub(super) at: String,
    pub(super) record_type: String,
    pub(super) name: String,
}

/// What reading an input produced.
#[derive(Debug, Default)]
pub(super) struct Imported {
    pub(super) desired: Vec<Desired>,
    pub(super) skipped: Vec<Skipped>,
}

fn blank_opts(ttl: i64) -> RecordOptions {
    RecordOptions {
        ttl: Some(ttl),
        priority: None,
        port: None,
        weight: None,
        protocol: None,
        service: None,
        flag: None,
        tag: None,
        usage: None,
        selector: None,
        matching_type: None,
        parameters: None,
    }
}

fn domain_name(domain: &str) -> Result<Name, String> {
    Name::parse(&format!("{domain}."), None)
        .map_err(|e| format!("`{domain}` is not a valid domain name: {}", e.message()))
}

// ---------------------------------------------------------------- zone file in

/// The options every import parses zone files with: relative names are under the
/// domain, and a record without a TTL gets the CLI's default. No custom record
/// types are registered: the API accepts only the standard ones, so a zone file
/// with any other type fails with its line number.
fn parse_options(origin: Name) -> ParseOptions {
    ParseOptions::default()
        .with_origin(origin)
        .with_default_ttl(u32::try_from(DEFAULT_TTL).unwrap_or(3600))
}

/// `name` relative to `origin`, as the text a `--name` flag would take: `@`, `www`,
/// `_sip._tcp.chat`. A name with non-ASCII labels comes back in Unicode, not as the
/// `\DDD` byte escapes the crate writes, so that it can be converted to punycode
/// like any other name. `None` when the name is not at or below `origin`.
fn relative_name(name: &Name, origin: &Name) -> Option<String> {
    let relative = name.relative_to(origin)?;
    if name.labels().iter().all(|label| label.is_ascii()) {
        return Some(relative);
    }
    let keep = name.labels().len() - origin.labels().len();
    let labels: Vec<String> = name.labels()[..keep]
        .iter()
        .map(|label| String::from_utf8_lossy(label).into_owned())
        .collect();
    Some(if labels.is_empty() {
        "@".to_owned()
    } else {
        labels.join(".")
    })
}

/// Read a zone file for `domain`. Every problem found is returned, not only the
/// first, so one run shows what to fix.
pub(super) fn from_zone_text(text: &str, domain: &str) -> Result<Imported, Vec<String>> {
    let origin = domain_name(domain).map_err(|e| vec![e])?;
    let zone = rfc1035_format::parse(text, &parse_options(origin.clone()))
        .map_err(|e| vec![e.to_string()])?;
    let mut imported = Imported::default();
    let mut errors = Vec::new();
    for record in &zone.records {
        let at = format!("line {}", record.line);
        let Some(relative) = relative_name(&record.name, &origin) else {
            errors.push(format!(
                "{at}: {} is not in {domain}. To import a zone file written for another \
                 domain, change its `$ORIGIN` line to `$ORIGIN {domain}.`",
                record.name
            ));
            continue;
        };
        let relative = relative.to_ascii_lowercase();
        if matches!(record.data, RData::Ns(_) | RData::Soa(_)) {
            imported.skipped.push(Skipped {
                at,
                record_type: record.data.type_name().to_owned(),
                name: relative,
            });
            continue;
        }
        match from_rdata(&relative, record) {
            Ok((record_type, name, value, opts)) => imported.desired.push(Desired {
                at,
                record_type,
                name,
                value,
                opts,
            }),
            Err(message) => errors.push(format!("{at}: {message}")),
        }
    }
    if errors.is_empty() {
        Ok(imported)
    } else {
        Err(errors)
    }
}

/// The `(type, name, value, options)` that `dns add` would build `record` from.
fn from_rdata(
    relative: &str,
    record: &Record,
) -> Result<(String, String, String, RecordOptions), String> {
    let mut opts = blank_opts(i64::from(record.ttl));
    let text = |bytes: &[u8], what: &str| {
        String::from_utf8(bytes.to_vec()).map_err(|_| format!("{what} is not valid UTF-8 text"))
    };
    let (record_type, name, value) = match &record.data {
        RData::A(ip) => ("A", relative.to_owned(), ip.to_string()),
        RData::Aaaa(ip) => ("AAAA", relative.to_owned(), ip.to_string()),
        RData::Cname(target) => ("CNAME", relative.to_owned(), target.to_string()),
        RData::Mx {
            preference,
            exchange,
        } => {
            opts.priority = Some(i64::from(*preference));
            ("MX", relative.to_owned(), exchange.to_string())
        }
        RData::Txt(parts) => (
            "TXT",
            relative.to_owned(),
            text(&parts.concat(), "the TXT value")?,
        ),
        RData::Srv {
            priority,
            weight,
            port,
            target,
        } => {
            let (service, protocol, name) = split_service(relative, "SRV", "_service._protocol")?;
            opts.priority = Some(i64::from(*priority));
            opts.weight = Some(i64::from(*weight));
            opts.port = Some(i64::from(*port));
            opts.service = Some(service);
            opts.protocol = Some(protocol);
            ("SRV", name, target.to_string())
        }
        RData::Caa { flags, tag, value } => {
            opts.flag = Some(i64::from(*flags));
            opts.tag = Some(tag.clone());
            ("CAA", relative.to_owned(), text(value, "the CAA value")?)
        }
        RData::Tlsa {
            usage,
            selector,
            matching_type,
            data,
        } => {
            let (port, protocol, name) = split_service(relative, "TLSA", "_port._protocol")?;
            let port = port
                .strip_prefix('_')
                .and_then(|p| p.parse::<u16>().ok())
                .filter(|p| *p > 0)
                .ok_or_else(|| {
                    format!(
                        "TLSA name `{relative}` must start with `_<port>`, as in `_443._tcp.www`"
                    )
                })?;
            opts.port = Some(i64::from(port));
            opts.protocol = Some(protocol);
            opts.usage = Some(i64::from(*usage));
            opts.selector = Some(i64::from(*selector));
            opts.matching_type = Some(i64::from(*matching_type));
            ("TLSA", name, hex(data))
        }
        RData::Https(svcb) | RData::Svcb(svcb) => {
            opts.priority = Some(i64::from(svcb.priority));
            opts.parameters = Some(svcb.params.clone()).filter(|p| !p.is_empty());
            (
                record.data.type_name(),
                relative.to_owned(),
                svcb.target.to_string(),
            )
        }
        other => {
            return Err(format!("{} records cannot be imported", other.type_name()));
        }
    };
    Ok((record_type.to_owned(), name, value, opts))
}

/// Split an SRV or TLSA owner (`_sip._tcp.chat`) into its two leading labels and
/// the record's own name (`@` when nothing follows).
fn split_service(
    relative: &str,
    record_type: &str,
    shape: &str,
) -> Result<(String, String, String), String> {
    let mut labels = relative.splitn(3, '.');
    match (labels.next(), labels.next()) {
        (Some(first), Some(second)) if first.starts_with('_') && second.starts_with('_') => Ok((
            first.to_owned(),
            second.to_owned(),
            labels.next().unwrap_or("@").to_owned(),
        )),
        _ => Err(format!(
            "{record_type} name `{relative}` must start with `{shape}`, as in `{}`",
            if record_type == "SRV" {
                "_sip._tcp.chat"
            } else {
                "_443._tcp.www"
            }
        )),
    }
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    })
}

// ------------------------------------------------------------------- JSON in

/// Read records written as `dns list -o json` writes them: the whole output (a
/// `data` list inside an envelope) or just the list.
pub(super) fn from_json_text(text: &str) -> Result<Imported, Vec<String>> {
    let value: Value = serde_json::from_str(text)
        .map_err(|e| vec![format!("the input is not valid JSON: {e}")])?;
    let list = match value {
        Value::Array(list) => list,
        Value::Object(mut envelope) => match envelope.remove("data") {
            Some(Value::Array(list)) => list,
            _ => return Err(vec![NOT_A_LIST.to_owned()]),
        },
        _ => return Err(vec![NOT_A_LIST.to_owned()]),
    };
    let mut imported = Imported::default();
    let mut errors = Vec::new();
    for (i, item) in list.into_iter().enumerate() {
        let at = format!("record {}", i + 1);
        let record: types::DnsRecord = match serde_json::from_value(item) {
            Ok(record) => record,
            Err(e) => {
                errors.push(format!("{at}: {e}"));
                continue;
            }
        };
        let record_type = record.type_.as_str().to_ascii_uppercase();
        if matches!(record_type.as_str(), "NS" | "SOA") {
            imported.skipped.push(Skipped {
                at,
                record_type,
                name: record.name.to_ascii_lowercase(),
            });
            continue;
        }
        let mut opts = opts_from_record(&record);
        let mut value = record_value(&record).unwrap_or_default().to_owned();
        if record_type == "TLSA" {
            // `dns list` shows a TLSA record's fields together in `data`
            // (`3 1 1 <hex>`), and leaves the separate fields out unless asked.
            if let Err(message) = split_merged_tlsa(&mut value, &mut opts) {
                errors.push(format!("{at}: {message}"));
                continue;
            }
        }
        imported.desired.push(Desired {
            at,
            value,
            name: record.name.to_ascii_lowercase(),
            opts,
            record_type,
        });
    }
    if errors.is_empty() {
        Ok(imported)
    } else {
        Err(errors)
    }
}

/// Take `usage selector matching-type certificate` apart when `value` is that
/// merged form and the separate fields are not set; leave a bare certificate alone.
fn split_merged_tlsa(value: &mut String, opts: &mut RecordOptions) -> Result<(), String> {
    if opts.usage.is_some() && opts.selector.is_some() && opts.matching_type.is_some() {
        return Ok(());
    }
    let parts: Vec<&str> = value.split_whitespace().collect();
    let [usage, selector, matching_type, cert] = parts.as_slice() else {
        return Err(
            "a TLSA record needs its usage, selector, matching type and certificate data"
                .to_owned(),
        );
    };
    let number = |text: &str, what: &str| {
        text.parse::<i64>()
            .map_err(|_| format!("TLSA {what} `{text}` is not a number"))
    };
    opts.usage = Some(number(usage, "usage")?);
    opts.selector = Some(number(selector, "selector")?);
    opts.matching_type = Some(number(matching_type, "matching type")?);
    let cert = (*cert).to_owned();
    *value = cert;
    Ok(())
}

const NOT_A_LIST: &str =
    "expected a list of DNS records, such as the output of `gddy dns list <domain> -o json`";

fn opts_from_record(record: &types::DnsRecord) -> RecordOptions {
    RecordOptions {
        ttl: Some(record.ttl),
        priority: record.priority.map(i64::from),
        port: record.port.map(i64::from),
        weight: record.weight.map(i64::from),
        protocol: record.protocol.clone(),
        service: record.service.clone(),
        flag: record.flag.map(i64::from),
        tag: record.tag.clone(),
        usage: record.usage.as_ref().map(|u| i64::from(u.0)),
        selector: record.selector.as_ref().map(|s| i64::from(s.0)),
        matching_type: record.matching_type.as_ref().map(|m| i64::from(m.0)),
        parameters: record.parameters.clone(),
    }
}

// ------------------------------------------------------------- zone file out

/// The value GoDaddy writes in place of an address when it has parked a domain.
/// The API refuses to take it from a client, so nothing can put it back.
const PARKED_MARKER: &str = "Parked";

/// Whether the DNS provider manages `record` itself, so an export leaves it out:
/// the SOA, the NS records at the domain itself (`@`), and the `A` or `AAAA` record
/// holding [`PARKED_MARKER`]. Every provider makes its own SOA and apex NS records
/// and ignores or rejects the ones in an import, and the parked marker can only be
/// written by GoDaddy, so a copy has no use for any of them. An NS record anywhere
/// else delegates a subdomain to other name servers, which is the domain owner's
/// data, so it stays.
pub(super) fn is_provider_managed(record: &types::DnsRecord) -> bool {
    match record.type_.as_str().to_ascii_uppercase().as_str() {
        "SOA" => true,
        "NS" => record.name.trim() == "@",
        "A" | "AAAA" => record
            .data
            .as_deref()
            .is_some_and(|data| data.trim().eq_ignore_ascii_case(PARKED_MARKER)),
        _ => false,
    }
}

/// The record types a zone file can hold and `dns import` writes. The API's record
/// type is a free string, so one it adds later, or one it should not return, would
/// otherwise stop an export; see [`is_nonstandard`].
const STANDARD_TYPES: &[&str] = &[
    "A", "AAAA", "CAA", "CNAME", "HTTPS", "MX", "NS", "SOA", "SRV", "SVCB", "TLSA", "TXT",
];

/// Whether `record` is something a standard DNS provider cannot use: a type outside
/// [`STANDARD_TYPES`] (the API's type is a free string), or an A or AAAA record whose
/// value is not an address. An address is read exactly as [`to_record`] reads it, so
/// a record this accepts is one an export can write. The parked marker is also such
/// a value, but an export never reaches this with it: see [`is_provider_managed`].
pub(super) fn is_nonstandard(record: &types::DnsRecord) -> bool {
    let data = record.data.as_deref().unwrap_or("");
    match record.type_.as_str().to_ascii_uppercase().as_str() {
        "A" => data.parse::<Ipv4Addr>().is_err(),
        "AAAA" => data.parse::<Ipv6Addr>().is_err(),
        other => !STANDARD_TYPES.contains(&other),
    }
}

/// Split `records` into those any DNS provider understands and those only GoDaddy
/// does (see [`is_nonstandard`]), each keeping its order.
pub(super) fn split_nonstandard(
    records: &[types::DnsRecord],
) -> (Vec<types::DnsRecord>, Vec<types::DnsRecord>) {
    records.iter().cloned().partition(|r| !is_nonstandard(r))
}

/// `A @ placeholder`: a record's type, name and value, for notes.
pub(super) fn describe_record(record: &types::DnsRecord) -> String {
    format!(
        "{} {} {}",
        record.type_.as_str(),
        record.name,
        record_value(record).unwrap_or("(no data)")
    )
}

/// The comment lines that say which records an export left out, with what each
/// held, so whoever moves the zone knows what to set up again. A record that is
/// not left out and cannot be written stops the export instead, so a file never
/// holds a record it could not use.
fn left_out_note(left_out: &[types::DnsRecord]) -> Vec<String> {
    if left_out.is_empty() {
        return Vec::new();
    }
    let mut lines = vec![format!(
        "; Left out ({} record(s) a standard DNS provider can't use); set them up again with your new provider:",
        left_out.len()
    )];
    lines.extend(
        left_out
            .iter()
            .map(|r| format!(";   {}", describe_record(r))),
    );
    lines
}

/// The records of `domain` as a zone file. Records that cannot be written as one
/// are an error naming each. Everything in `left_out` (records the caller chose not
/// to export) is left out, and the file says so, with what each held, in comments.
/// The caller drops what the provider manages (see [`is_provider_managed`]).
pub(super) fn export_zone(
    records: &[types::DnsRecord],
    domain: &str,
    left_out: &[types::DnsRecord],
) -> Result<String, Vec<String>> {
    let origin = domain_name(domain).map_err(|e| vec![e])?;
    let mut out = Vec::new();
    let notes = left_out_note(left_out);
    let mut errors = Vec::new();
    for record in records {
        match to_record(record, &origin, domain) {
            Ok(converted) => out.push(converted),
            Err(message) => errors.push(message),
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    // Stable, so records of one name and type keep the API's order; the domain's
    // own records first, then the rest by name.
    out.sort_by_cached_key(|record| {
        let owner = record
            .name
            .relative_to(&origin)
            .unwrap_or_else(|| record.name.to_string());
        (owner != "@", owner, record.data.type_name().to_owned())
    });
    let zone = Zone {
        origin: Some(origin),
        records: out,
    };
    let mut text = String::new();
    for note in notes {
        text.push_str(&note);
        text.push('\n');
    }
    text.push_str(&zone.to_zone_file());
    Ok(text)
}

/// One API record as a zone-file record.
fn to_record(record: &types::DnsRecord, origin: &Name, domain: &str) -> Result<Record, String> {
    let record_type = record.type_.as_str().to_ascii_uppercase();
    let fail = |why: &str| {
        format!(
            "{record_type} {} {}: {why}",
            record.name,
            record_value(record).unwrap_or("(no data)")
        )
    };
    let owner =
        composed_owner(record).ok_or_else(|| fail("it has no service, protocol or port"))?;
    let name = if owner == "@" {
        origin.clone()
    } else {
        Name::parse(&format!("{owner}.{domain}."), None).map_err(|e| fail(e.message()))?
    };
    let ttl = u32::try_from(record.ttl).map_err(|_| fail("its TTL is out of range"))?;
    let data = record.data.as_deref().unwrap_or("");
    let target = || absolute_name(data).map_err(|e| fail(&e));
    let need =
        |value: Option<u16>, field: &str| value.ok_or_else(|| fail(&format!("it has no {field}")));
    let rdata = match record_type.as_str() {
        "A" => RData::A(
            data.parse::<Ipv4Addr>()
                .map_err(|_| fail("the value is not an IPv4 address"))?,
        ),
        "AAAA" => RData::Aaaa(
            data.parse::<Ipv6Addr>()
                .map_err(|_| fail("the value is not an IPv6 address"))?,
        ),
        "CNAME" => RData::Cname(target()?),
        "NS" => RData::Ns(target()?),
        "MX" => RData::Mx {
            preference: need(record.priority, "priority")?,
            exchange: target()?,
        },
        "TXT" => RData::Txt(if data.is_empty() {
            vec![Vec::new()]
        } else {
            data.as_bytes().chunks(255).map(<[u8]>::to_vec).collect()
        }),
        "SRV" => RData::Srv {
            priority: need(record.priority, "priority")?,
            weight: need(record.weight, "weight")?,
            port: need(record.port, "port")?,
            target: target()?,
        },
        "CAA" => RData::Caa {
            flags: record.flag.unwrap_or(0),
            tag: record.tag.clone().ok_or_else(|| fail("it has no tag"))?,
            value: data.as_bytes().to_vec(),
        },
        "TLSA" => {
            let byte = |field: &Option<u8>, what: &str| {
                field.ok_or_else(|| fail(&format!("it has no {what}")))
            };
            RData::Tlsa {
                usage: byte(&record.usage.as_ref().map(|v| v.0), "usage")?,
                selector: byte(&record.selector.as_ref().map(|v| v.0), "selector")?,
                matching_type: byte(&record.matching_type.as_ref().map(|v| v.0), "matching type")?,
                data: decode_hex(record.certificate_data.as_deref().unwrap_or(""))
                    .map_err(|e| fail(&e))?,
            }
        }
        "HTTPS" | "SVCB" => {
            let svcb = Svcb {
                priority: need(record.priority, "priority")?,
                target: target()?,
                params: record.parameters.clone().unwrap_or_default(),
            };
            if record_type == "HTTPS" {
                RData::Https(svcb)
            } else {
                RData::Svcb(svcb)
            }
        }
        other => {
            return Err(fail(&format!(
                "{other} records cannot be written to a zone file"
            )));
        }
    };
    Ok(Record {
        name,
        ttl,
        data: rdata,
        line: 0,
    })
}

/// A hostname the API returns as an absolute name, with or without its final dot.
/// Never relative to the zone: `mail.example.com` is that host, not
/// `mail.example.com.example.com.`.
fn absolute_name(text: &str) -> Result<Name, String> {
    if text.is_empty() {
        return Err("the value is empty".to_owned());
    }
    let absolute = if text.ends_with('.') {
        text.to_owned()
    } else {
        format!("{text}.")
    };
    Name::parse(&absolute, None).map_err(|e| e.message().to_owned())
}

fn decode_hex(hex: &str) -> Result<Vec<u8>, String> {
    if hex.is_empty() || !hex.len().is_multiple_of(2) {
        return Err("the certificate data is not whole bytes of hex".to_owned());
    }
    hex.as_bytes()
        .chunks(2)
        .map(|pair| {
            std::str::from_utf8(pair)
                .ok()
                .and_then(|digits| u8::from_str_radix(digits, 16).ok())
                .ok_or_else(|| "the certificate data is not hex".to_owned())
        })
        .collect()
}

#[cfg(test)]
#[path = "zone_records_tests.rs"]
mod tests;
