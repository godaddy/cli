//! Writes a [`Zone`] as master file text.
//!
//! Every record is written in full (`name ttl IN type data`) with no `$TTL` and
//! no inherited owners, so a line means the same wherever it is copied to. Names
//! inside the data are absolute; owner names are relative to the zone's origin.

use std::fmt::Write as _;

use crate::name::Name;
use crate::rdata::{CustomValue, RData};
use crate::zone::{Record, Zone};

pub(crate) fn write_zone(zone: &Zone) -> String {
    let mut out = String::new();
    if let Some(origin) = &zone.origin {
        let _ = writeln!(out, "$ORIGIN {origin}");
    }
    for record in &zone.records {
        write_record(&mut out, record, zone.origin.as_ref());
    }
    out
}

fn write_record(out: &mut String, record: &Record, origin: Option<&Name>) {
    let owner = origin
        .and_then(|o| record.name.relative_to(o))
        .unwrap_or_else(|| record.name.to_string());
    let data = rdata_text(&record.data);
    let _ = write!(out, "{owner} {} IN {}", record.ttl, record.data.type_name());
    if !data.is_empty() {
        out.push(' ');
        out.push_str(&data);
    }
    out.push('\n');
}

/// The data of a record as it follows the type in a zone file.
pub(crate) fn rdata_text(data: &RData) -> String {
    match data {
        RData::A(address) => address.to_string(),
        RData::Aaaa(address) => address.to_string(),
        RData::Ns(name) | RData::Cname(name) => name.to_string(),
        RData::Mx {
            preference,
            exchange,
        } => format!("{preference} {exchange}"),
        RData::Txt(strings) => strings
            .iter()
            .map(|s| quote(s))
            .collect::<Vec<_>>()
            .join(" "),
        RData::Srv {
            priority,
            weight,
            port,
            target,
        } => format!("{priority} {weight} {port} {target}"),
        RData::Caa { flags, tag, value } => format!("{flags} {tag} {}", quote(value)),
        RData::Tlsa {
            usage,
            selector,
            matching_type,
            data,
        } => format!("{usage} {selector} {matching_type} {}", hex(data)),
        RData::Https(svcb) | RData::Svcb(svcb) => {
            let head = format!("{} {}", svcb.priority, svcb.target);
            if svcb.params.is_empty() {
                head
            } else {
                format!("{head} {}", svcb.params)
            }
        }
        RData::Soa(soa) => format!(
            "{} {} {} {} {} {} {}",
            soa.mname, soa.rname, soa.serial, soa.refresh, soa.retry, soa.expire, soa.minimum
        ),
        RData::Custom { value, .. } => match value {
            CustomValue::Name(name) => name.to_string(),
            CustomValue::Words(words) => words.join(" "),
        },
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    })
}

/// `bytes` as a quoted string that reads back to the same bytes. Valid UTF-8
/// stays readable; control characters and invalid bytes become `\DDD`.
pub(crate) fn quote(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() + 2);
    out.push('"');
    match std::str::from_utf8(bytes) {
        Ok(text) => {
            for ch in text.chars() {
                match ch {
                    '"' | '\\' => {
                        out.push('\\');
                        out.push(ch);
                    }
                    c if c.is_control() => {
                        // `\DDD` is one byte, so a character that takes several
                        // bytes in UTF-8 (U+0085 is `C2 85`) needs one escape each.
                        let mut buf = [0u8; 4];
                        for byte in c.encode_utf8(&mut buf).bytes() {
                            let _ = write!(out, "\\{byte:03}");
                        }
                    }
                    c => out.push(c),
                }
            }
        }
        Err(_) => {
            for &byte in bytes {
                match byte {
                    b'"' | b'\\' => {
                        out.push('\\');
                        out.push(char::from(byte));
                    }
                    0x20..=0x7e => out.push(char::from(byte)),
                    _ => {
                        let _ = write!(out, "\\{byte:03}");
                    }
                }
            }
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
#[path = "writer_tests.rs"]
mod tests;
