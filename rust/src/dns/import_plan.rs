//! Planning for `dns import`: checking the records read from the input, grouping
//! them by type and name, and deciding what to create, change or delete. Pure
//! (no I/O), so the decisions are unit-testable.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::net::{Ipv4Addr, Ipv6Addr};

use domains_client::types;
use rfc1035_format::Name;

use super::add::{add_blocker, at_owner};
use super::records::{
    MAX_TTL, MIN_TTL, WRITABLE_TYPES, composed_owner, parse_underscore_label, same_content,
    validate_applicable_fields, validate_caa_fields, validate_record_name, validate_svcb_fields,
    validate_tlsa_fields,
};
use super::set::{SetAction, plan_set_actions};
use super::zone_records::{Desired, decode_hex};

/// The records of one type under one name: what the input wants there, what is
/// there now, and what to do about it.
#[derive(Debug)]
pub(super) struct Group {
    pub(super) record_type: String,
    /// The name the API files the records under, lower-case (see `composed_owner`).
    pub(super) owner: String,
    /// The records' own name, as `dns add --name` takes it.
    pub(super) name: String,
    pub(super) desired: Vec<Desired>,
    pub(super) existing: Vec<types::DnsRecord>,
    pub(super) actions: Vec<SetAction>,
    /// Creates that would be refused, by position in `desired`, and why.
    pub(super) blocked: Vec<(usize, String)>,
}

impl Group {
    /// The options of each desired record, indexed like `desired`.
    pub(super) fn opts(&self) -> Vec<super::records::RecordOptions> {
        self.desired.iter().map(|d| d.opts.clone()).collect()
    }
}

#[derive(Debug)]
pub(super) struct ImportPlan {
    pub(super) groups: Vec<Group>,
    /// Things the user should know that do not stop the import.
    pub(super) warnings: Vec<String>,
}

fn in_range(field: &str, value: Option<i64>, min: i64, max: i64) -> Result<(), String> {
    match value {
        Some(n) if !(min..=max).contains(&n) => {
            Err(format!("the {field} {n} must be from {min} to {max}"))
        }
        _ => Ok(()),
    }
}

/// Check one record the way `dns add` checks its flags, since an import reaches
/// the API without passing through them.
fn check(d: &mut Desired) -> Result<(), String> {
    d.record_type = d.record_type.to_ascii_uppercase();
    let record_type = d.record_type.clone();
    if !WRITABLE_TYPES.contains(&record_type.as_str()) {
        return Err(format!(
            "{record_type} records cannot be imported; importable types: {}",
            WRITABLE_TYPES.join(", ")
        ));
    }
    // A zone file's records are read as addresses already; JSON's are plain text,
    // so check both here, before a plan can delete anything on their account.
    match record_type.as_str() {
        "A" if d.value.parse::<Ipv4Addr>().is_err() => {
            return Err(format!("the value `{}` is not an IPv4 address", d.value));
        }
        // An IPv6 address has many spellings (`2001:DB8::1`, `2001:db8:0::1`); the
        // zone file writer uses one, so compare in that form.
        "AAAA" => match d.value.parse::<Ipv6Addr>() {
            Ok(address) => d.value = address.to_string(),
            Err(_) => return Err(format!("the value `{}` is not an IPv6 address", d.value)),
        },
        // `.` is a valid HTTPS or SVCB target, but nothing is not a hostname.
        "CNAME" | "MX" | "SRV" | "HTTPS" | "SVCB" if d.value.trim().is_empty() => {
            return Err(format!("{record_type} records need a target name"));
        }
        "TLSA" => {
            decode_hex(&d.value)?;
        }
        _ => {}
    }
    // A target in Unicode (`café.example.net.`, as a zone file may spell it) is the
    // punycode target the API holds, so compare it in that form. The root `.` and a
    // trailing dot are kept.
    if matches!(
        record_type.as_str(),
        "CNAME" | "MX" | "SRV" | "HTTPS" | "SVCB"
    ) {
        d.value = crate::idn::parse_name_arg(&d.value)?;
    }
    // `dns list` shows a name in Unicode and the API holds it as punycode, so
    // compare in the form the API uses, as a name typed on the command line is.
    d.name = crate::idn::parse_name_arg(&d.name)?;
    let ttl = d.ttl();
    if !(MIN_TTL..=MAX_TTL).contains(&ttl) {
        return Err(format!(
            "the TTL {ttl} is outside the {MIN_TTL} to {MAX_TTL} seconds DNS records here allow; \
             change it in the input"
        ));
    }
    let o = &d.opts;
    in_range("priority", o.priority, 0, 65_535)?;
    in_range("weight", o.weight, 0, 65_535)?;
    // Port 0 is valid for an SRV record (RFC 2782: with a target of `.`, the service
    // is unavailable). TLSA keeps the 1 to 65535 range `dns add` gives its `--port`.
    in_range("port", o.port, i64::from(record_type != "SRV"), 65_535)?;
    in_range("flag", o.flag, 0, 255)?;
    in_range("usage", o.usage, 0, 3)?;
    in_range("selector", o.selector, 0, 1)?;
    in_range("matching type", o.matching_type, 0, 2)?;

    let needs: [(&str, bool, &[&str]); 9] = [
        (
            "priority",
            o.priority.is_some(),
            &["MX", "SRV", "HTTPS", "SVCB"],
        ),
        ("weight", o.weight.is_some(), &["SRV"]),
        ("port", o.port.is_some(), &["SRV", "TLSA"]),
        ("service", o.service.is_some(), &["SRV"]),
        ("protocol", o.protocol.is_some(), &["SRV", "TLSA"]),
        (
            "tag",
            o.tag.as_deref().is_some_and(|t| !t.trim().is_empty()),
            &["CAA"],
        ),
        ("usage", o.usage.is_some(), &["TLSA"]),
        ("selector", o.selector.is_some(), &["TLSA"]),
        ("matching type", o.matching_type.is_some(), &["TLSA"]),
    ];
    for (field, present, types) in needs {
        if types.contains(&record_type.as_str()) && !present {
            return Err(format!("{record_type} records need a {field}"));
        }
    }

    // `sip` and `_sip` mean the same service label.
    for label in [&mut d.opts.service, &mut d.opts.protocol] {
        if let Some(raw) = label.take() {
            *label = Some(parse_underscore_label(&raw)?);
        }
    }
    validate_record_name(&record_type, &d.name)?;
    validate_applicable_fields(&record_type, &d.opts)?;
    validate_caa_fields(&record_type, &d.opts)?;
    validate_tlsa_fields(&record_type, &d.opts)?;
    validate_svcb_fields(&record_type, &d.opts)
}

/// Check every record, returning one message per problem, each naming where in
/// the input it is, so a single run shows everything to fix.
pub(super) fn validate(desired: &mut [Desired]) -> Vec<String> {
    desired
        .iter_mut()
        .filter_map(|d| check(d).err().map(|problem| format!("{}: {problem}", d.at)))
        .collect()
}

/// `records` as they should be compared with the input: an AAAA address written in
/// the one form [`check`] gives the input's. The originals are kept for showing and
/// writing, since the API stores the text as it was given.
fn comparable_addresses(records: &[types::DnsRecord]) -> Vec<types::DnsRecord> {
    records
        .iter()
        .map(|record| {
            let mut record = record.clone();
            if record.type_.as_str().eq_ignore_ascii_case("AAAA")
                && let Some(address) = record
                    .data
                    .as_deref()
                    .and_then(|data| data.parse::<Ipv6Addr>().ok())
            {
                record.data = Some(address.to_string());
            }
            record
        })
        .collect()
}

/// The action for each desired record when nothing is to be removed: an
/// identical record is left alone (or has its TTL brought into line), anything
/// else is created.
fn plan_additive(existing: &[types::DnsRecord], desired: &[Desired]) -> Vec<SetAction> {
    let mut taken = vec![false; existing.len()];
    desired
        .iter()
        .enumerate()
        .map(|(at, d)| {
            let want = d.record();
            let found =
                (0..existing.len()).find(|&e| !taken[e] && same_content(&existing[e], &want));
            match found {
                Some(e) => {
                    taken[e] = true;
                    let record_id = existing[e].record_id.clone().unwrap_or_default();
                    if existing[e].ttl == d.ttl() {
                        SetAction::Unchanged {
                            record_id,
                            data: d.value.clone(),
                            desired: at,
                        }
                    } else {
                        SetAction::Replace {
                            record_id,
                            data: d.value.clone(),
                            desired: at,
                        }
                    }
                }
                None => SetAction::Create {
                    data: d.value.clone(),
                    desired: at,
                },
            }
        })
        .collect()
}

fn owner_of(record: &types::DnsRecord) -> String {
    composed_owner(record)
        .unwrap_or_else(|| record.name.clone())
        .to_ascii_lowercase()
}

/// Group `desired` by type and owner, pair each group with what is in the zone,
/// and plan it. With `replace`, anything in the zone that the input does not
/// have is deleted; without it nothing is. `existing` is the whole zone.
pub(super) fn build_plan(
    domain: &str,
    desired: Vec<Desired>,
    existing: &[types::DnsRecord],
    replace: bool,
) -> Result<ImportPlan, Vec<String>> {
    // Only the types an import can write are ever compared with or removed;
    // GoDaddy's NS and SOA records stay out of it.
    let managed: Vec<&types::DnsRecord> = existing
        .iter()
        .filter(|r| WRITABLE_TYPES.contains(&r.type_.as_str()))
        .collect();
    if managed.iter().any(|r| r.record_id.is_none()) {
        return Err(vec![format!(
            "some records of {domain} were returned without an ID, so they cannot be compared \
             safely; run `gddy dns list {domain}` and try again"
        )]);
    }

    // The whole owner has to be a name the zone can hold: no empty label, no label
    // over 63 bytes, nothing longer than a DNS name may be. This needs the domain, so
    // it is checked here, still before anything is planned or deleted.
    let owner_errors: Vec<String> = desired
        .iter()
        .filter_map(|d| {
            let owner = d.owner();
            if owner == "@" {
                return None;
            }
            Name::parse(&format!("{owner}.{domain}."), None)
                .err()
                .map(|e| {
                    format!(
                        "{}: `{owner}` is not a valid name in {domain}: {}",
                        d.at,
                        e.message()
                    )
                })
        })
        .collect();
    if !owner_errors.is_empty() {
        return Err(owner_errors);
    }

    let mut warnings = Vec::new();
    let mut groups: Vec<Group> = Vec::new();
    let mut index: HashMap<(String, String), usize> = HashMap::new();
    for d in desired {
        let key = (d.record_type.clone(), d.owner());
        let at = match index.get(&key) {
            Some(&at) => at,
            None => {
                groups.push(Group {
                    record_type: d.record_type.clone(),
                    owner: key.1.clone(),
                    name: d.name.clone(),
                    desired: Vec::new(),
                    existing: Vec::new(),
                    actions: Vec::new(),
                    blocked: Vec::new(),
                });
                index.insert(key, groups.len() - 1);
                groups.len() - 1
            }
        };
        let group = &mut groups[at];
        let record = d.record();
        // The TTL counts: the same record twice with different TTLs is a conflict to
        // report below, not a repeat to drop.
        if let Some(first) = group
            .desired
            .iter()
            .find(|earlier| earlier.ttl() == d.ttl() && same_content(&earlier.record(), &record))
        {
            warnings.push(format!(
                "{}: ignored, it repeats {} ({})",
                d.at,
                d.label(),
                first.at
            ));
            continue;
        }
        group.desired.push(d);
    }

    // A record set has one TTL, so a group whose records disagree is a mistake in
    // the input rather than something to pick a side on.
    let mut errors = Vec::new();
    for group in &groups {
        let ttls: BTreeSet<i64> = group.desired.iter().map(Desired::ttl).collect();
        if ttls.len() > 1 {
            errors.push(format!(
                "{} records for {} have different TTLs ({}); give them all one TTL ({})",
                group.record_type,
                group.name,
                ttls.iter()
                    .map(i64::to_string)
                    .collect::<Vec<_>>()
                    .join(", "),
                group
                    .desired
                    .iter()
                    .map(|d| d.at.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }

    for group in &mut groups {
        group.existing = managed
            .iter()
            .filter(|r| r.type_.as_str() == group.record_type && owner_of(r) == group.owner)
            .map(|r| (*r).clone())
            .collect();
        let comparable = comparable_addresses(&group.existing);
        group.actions = if replace {
            let records: Vec<types::DnsRecord> =
                group.desired.iter().map(Desired::record).collect();
            let ttl = group.desired.first().map(Desired::ttl);
            plan_set_actions(&comparable, &records, ttl)
        } else {
            plan_additive(&comparable, &group.desired)
        };
    }

    if replace {
        // What the zone has and the input does not.
        let mut absent: Vec<Group> = Vec::new();
        let mut absent_index: HashMap<(String, String), usize> = HashMap::new();
        for record in &managed {
            let key = (record.type_.as_str().to_owned(), owner_of(record));
            if index.contains_key(&key) {
                continue;
            }
            let at = *absent_index.entry(key.clone()).or_insert_with(|| {
                absent.push(Group {
                    record_type: key.0.clone(),
                    owner: key.1.clone(),
                    name: record.name.clone(),
                    desired: Vec::new(),
                    existing: Vec::new(),
                    actions: Vec::new(),
                    blocked: Vec::new(),
                });
                absent.len() - 1
            });
            absent[at].actions.push(SetAction::Delete {
                record_id: record.record_id.clone().unwrap_or_default(),
            });
            absent[at].existing.push((*record).clone());
        }
        groups.extend(absent);
    }

    block_refused_creates(domain, &mut groups, existing);
    Ok(ImportPlan { groups, warnings })
}

/// Take out of each group the creates the API would refuse (a second CNAME, a
/// CNAME next to other records), judged against the zone as the import would leave
/// it: the existing records without those it deletes, plus the creates before it.
fn block_refused_creates(domain: &str, groups: &mut [Group], existing: &[types::DnsRecord]) {
    let deleted: HashSet<&str> = groups
        .iter()
        .flat_map(|g| &g.actions)
        .filter_map(|a| match a {
            SetAction::Delete { record_id } => Some(record_id.as_str()),
            _ => None,
        })
        .collect();
    let mut simulated: Vec<types::DnsRecord> = existing
        .iter()
        .filter(|r| {
            r.record_id
                .as_deref()
                .is_none_or(|id| !deleted.contains(id))
        })
        .cloned()
        .collect();
    for group in groups.iter_mut() {
        let mut kept = Vec::with_capacity(group.actions.len());
        for action in std::mem::take(&mut group.actions) {
            if let SetAction::Create { desired, .. } = &action {
                let d = &group.desired[*desired];
                let here: Vec<types::DnsRecord> = simulated
                    .iter()
                    .filter(|r| at_owner(r, &group.owner))
                    .cloned()
                    .collect();
                if let Some(reason) =
                    add_blocker(domain, &d.name, &d.record_type, &d.value, &d.opts, &here)
                {
                    group.blocked.push((*desired, reason));
                    continue;
                }
                simulated.push(d.record());
            }
            kept.push(action);
        }
        group.actions = kept;
    }
}

#[cfg(test)]
#[path = "import_plan_tests.rs"]
mod tests;
