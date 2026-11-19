//! Turns record attributes into template records. Attributes arrive either as
//! an `--add-<type>` flag value (`host=www,value=ip`) or one at a time as
//! prompt answers; both share the per-type field lists and value checks here.

use std::collections::HashMap;

use clap::ValueEnum;

use super::types::{Essential, Record, RecordKind};
use crate::error::GddyError;

const SYSTEM: &str = "domain-connect";

/// An attribute a record can be given, written `key=value` in a flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Attr {
    Host,
    Value,
    Data,
    Rules,
    Target,
    Service,
    Protocol,
    Name,
    Priority,
    Weight,
    Port,
    Ttl,
    Mode,
    Prefix,
    Group,
    Essential,
}

impl Attr {
    pub(super) const fn key(self) -> &'static str {
        match self {
            Self::Host => "host",
            Self::Value => "value",
            Self::Data => "data",
            Self::Rules => "rules",
            Self::Target => "target",
            Self::Service => "service",
            Self::Protocol => "protocol",
            Self::Name => "name",
            Self::Priority => "priority",
            Self::Weight => "weight",
            Self::Port => "port",
            Self::Ttl => "ttl",
            Self::Mode => "mode",
            Self::Prefix => "prefix",
            Self::Group => "group",
            Self::Essential => "essential",
        }
    }
}

/// Whether a field can be left out, and what it then becomes.
pub(super) enum Need {
    Required,
    Optional,
    Default(&'static str),
}

/// One attribute of a record type, in the order it is asked for.
pub(super) struct Field {
    pub(super) attr: Attr,
    pub(super) label: &'static str,
    pub(super) need: Need,
}

const fn required(attr: Attr, label: &'static str) -> Field {
    Field {
        attr,
        label,
        need: Need::Required,
    }
}

const fn optional(attr: Attr, label: &'static str) -> Field {
    Field {
        attr,
        label,
        need: Need::Optional,
    }
}

const fn defaulted(attr: Attr, label: &'static str, default: &'static str) -> Field {
    Field {
        attr,
        label,
        need: Need::Default(default),
    }
}

const HOST: Field = defaulted(Attr::Host, "Host (@ is the root of the domain)", "@");
const TTL: Field = defaulted(Attr::Ttl, "TTL in seconds", "3600");
const GROUP: Field = optional(Attr::Group, "Group ID, to group related records");
const ESSENTIAL: Field = optional(Attr::Essential, "Essential (Always or OnApply)");

const A_FIELDS: &[Field] = &[
    HOST,
    required(Attr::Value, "IPv4 address, or a variable name such as ip"),
    TTL,
    GROUP,
    ESSENTIAL,
];
const AAAA_FIELDS: &[Field] = &[
    HOST,
    required(Attr::Value, "IPv6 address, or a variable name such as ip6"),
    TTL,
    GROUP,
    ESSENTIAL,
];
const ALIAS_FIELDS: &[Field] = &[
    HOST,
    required(Attr::Value, "Host name it points to"),
    TTL,
    GROUP,
    ESSENTIAL,
];
const NS_FIELDS: &[Field] = &[
    HOST,
    required(Attr::Value, "Name server host name"),
    TTL,
    GROUP,
    ESSENTIAL,
];
const MX_FIELDS: &[Field] = &[
    HOST,
    required(Attr::Value, "Mail server host name"),
    defaulted(Attr::Priority, "Priority", "10"),
    TTL,
    GROUP,
    ESSENTIAL,
];
const TXT_FIELDS: &[Field] = &[
    HOST,
    required(Attr::Data, "Text"),
    TTL,
    optional(Attr::Mode, "Conflict matching mode (None, All or Prefix)"),
    optional(Attr::Prefix, "Conflict matching prefix"),
    GROUP,
    ESSENTIAL,
];
const CAA_FIELDS: &[Field] = &[
    HOST,
    required(Attr::Data, "Value, such as 0 issue \"letsencrypt.org\""),
    TTL,
    GROUP,
    ESSENTIAL,
];
const SPFM_FIELDS: &[Field] = &[
    HOST,
    required(Attr::Rules, "SPF rules, such as include:_spf.example.com"),
    TTL,
    GROUP,
    ESSENTIAL,
];
const SRV_FIELDS: &[Field] = &[
    required(Attr::Service, "Service, such as _sip"),
    required(Attr::Protocol, "Protocol, such as _tcp"),
    defaulted(Attr::Name, "Name (@ is the root of the domain)", "@"),
    required(Attr::Target, "Target host name"),
    defaulted(Attr::Priority, "Priority", "10"),
    defaulted(Attr::Weight, "Weight", "1"),
    defaulted(Attr::Port, "Port", "80"),
    TTL,
    GROUP,
    ESSENTIAL,
];
const REDIRECT_FIELDS: &[Field] = &[
    HOST,
    required(Attr::Target, "URL to redirect to"),
    GROUP,
    ESSENTIAL,
];

pub(super) const fn fields(kind: RecordKind) -> &'static [Field] {
    match kind {
        RecordKind::A => A_FIELDS,
        RecordKind::Aaaa => AAAA_FIELDS,
        RecordKind::Cname | RecordKind::Apexcname => ALIAS_FIELDS,
        RecordKind::Ns => NS_FIELDS,
        RecordKind::Mx => MX_FIELDS,
        RecordKind::Txt => TXT_FIELDS,
        RecordKind::Caa => CAA_FIELDS,
        RecordKind::Spfm => SPFM_FIELDS,
        RecordKind::Srv => SRV_FIELDS,
        RecordKind::Redir301 | RecordKind::Redir302 => REDIRECT_FIELDS,
    }
}

/// Stores one attribute on `record`, checking numbers and fixed choices.
pub(super) fn set(
    record: &mut Record,
    kind: RecordKind,
    attr: Attr,
    value: String,
) -> Result<(), GddyError> {
    match attr {
        Attr::Host => record.host = Some(value),
        Attr::Name => record.name = Some(value),
        Attr::Value => {
            record.points_to = Some(match kind {
                RecordKind::A | RecordKind::Aaaa => address(value),
                _ => value,
            });
        }
        Attr::Target => record.target = Some(value),
        Attr::Data => record.data = Some(value),
        Attr::Rules => record.spf_rules = Some(value),
        Attr::Service => record.service = Some(value),
        Attr::Protocol => record.protocol = Some(value),
        Attr::Prefix => record.txt_conflict_matching_prefix = Some(value),
        Attr::Group => record.group_id = Some(value),
        Attr::Ttl => record.ttl = Some(number(attr, &value)?),
        Attr::Priority => record.priority = Some(number(attr, &value)?),
        Attr::Weight => record.weight = Some(number(attr, &value)?),
        Attr::Port => record.port = Some(number(attr, &value)?),
        Attr::Essential => record.essential = Some(choice(attr, &value)?),
        Attr::Mode => record.txt_conflict_matching_mode = Some(choice(attr, &value)?),
    }
    Ok(())
}

/// An IP address, or anything already written with `%variables%`, is kept as
/// given. A bare word is a variable name.
fn address(value: String) -> String {
    if value.contains('%') || value.parse::<std::net::IpAddr>().is_ok() {
        value
    } else {
        format!("%{value}%")
    }
}

fn number(attr: Attr, value: &str) -> Result<i32, GddyError> {
    value.parse().map_err(|_| {
        GddyError::validation(format!(
            "{} must be a whole number, got '{value}'",
            attr.key()
        ))
        .with_fix(format!("Give {} as digits only.", attr.key()))
        .with_system(SYSTEM)
    })
}

fn choice<T: ValueEnum>(attr: Attr, value: &str) -> Result<T, GddyError> {
    T::from_str(value, true).map_err(|_| {
        let allowed: Vec<String> = T::value_variants()
            .iter()
            .filter_map(T::to_possible_value)
            .map(|possible| possible.get_name().to_owned())
            .collect();
        GddyError::validation(format!("'{value}' is not a valid {} value", attr.key()))
            .with_fix(format!(
                "Set {} to one of: {}.",
                attr.key(),
                allowed.join(", ")
            ))
            .with_system(SYSTEM)
    })
}

/// Parses every value of one `--add-<type>` flag. `group` and `essential`
/// apply to records that do not set their own.
pub(super) fn parse_records(
    kind: RecordKind,
    inputs: &[String],
    group: Option<&str>,
    essential: Option<Essential>,
) -> cli_engine::Result<Vec<Record>> {
    inputs
        .iter()
        .map(|input| {
            let mut record = parse_record(kind, input)?;
            if record.group_id.is_none() {
                record.group_id = group.map(str::to_owned);
            }
            record.essential = record.essential.or(essential);
            Ok(record)
        })
        .collect()
}

pub(super) fn parse_record(kind: RecordKind, input: &str) -> cli_engine::Result<Record> {
    let mut given = split(kind, input)?;
    let mut record = Record::new(kind);
    for field in fields(kind) {
        // An empty value counts as left out, except where it replaces a default.
        let given = given
            .remove(&field.attr)
            .filter(|value| !value.is_empty() || matches!(field.need, Need::Default(_)));
        let value = match (given, &field.need) {
            (Some(value), _) => value,
            (None, Need::Default(default)) => (*default).to_owned(),
            (None, Need::Optional) => continue,
            (None, Need::Required) => {
                return Err(invalid(
                    kind,
                    format!("{} record requires '{}'", kind.name(), field.attr.key()),
                ));
            }
        };
        set(&mut record, kind, field.attr, value).map_err(GddyError::into_cli_error)?;
    }
    Ok(record)
}

fn split(kind: RecordKind, input: &str) -> cli_engine::Result<HashMap<Attr, String>> {
    let mut given: HashMap<Attr, String> = HashMap::new();
    let mut last: Option<Attr> = None;
    for part in input.split(',') {
        let named = part.split_once('=').and_then(|(key, value)| {
            fields(kind)
                .iter()
                .find(|field| field.attr.key() == key.trim())
                .map(|field| (field.attr, value))
        });
        match (named, last) {
            (Some((attr, value)), _) => {
                if given.insert(attr, value.to_owned()).is_some() {
                    return Err(invalid(
                        kind,
                        format!("'{}' is given more than once", attr.key()),
                    ));
                }
                last = Some(attr);
            }
            // `data` is free text, so a part that names no attribute is the
            // rest of a value that contains a comma.
            (None, Some(Attr::Data)) => {
                let text = given.entry(Attr::Data).or_default();
                text.push(',');
                text.push_str(part);
            }
            (None, _) => {
                return Err(invalid(
                    kind,
                    format!(
                        "'{}' is not an attribute of {} records",
                        part.trim(),
                        kind.name()
                    ),
                ));
            }
        }
    }
    for value in given.values_mut() {
        *value = value.trim().to_owned();
    }
    Ok(given)
}

fn invalid(kind: RecordKind, message: String) -> cli_engine::CliCoreError {
    let accepted: Vec<&str> = fields(kind).iter().map(|field| field.attr.key()).collect();
    GddyError::validation(message)
        .with_fix(format!(
            "Write --add-{} as key=value pairs separated by commas, using: {}.",
            kind.name().to_ascii_lowercase(),
            accepted.join(", ")
        ))
        .with_system(SYSTEM)
        .into_cli_error()
}

#[cfg(test)]
#[path = "parsers_tests.rs"]
mod tests;
