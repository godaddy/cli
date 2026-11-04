//! Final-state check for `dns set`: after the planned writes succeed, read the
//! type+name back and confirm the records now there are the ones that were
//! asked for. The plan is made from a single earlier read, so a record added or
//! changed by someone else in between would otherwise go unnoticed.

use cli_engine::CliCoreError;
use domains_client::types;

use crate::dns::records::{comparable, fetch_records, record_value};

/// What differs between the records asked for and the records now present.
#[derive(Debug, PartialEq)]
pub(super) struct Mismatch {
    /// Asked for, but not present.
    pub(super) missing: Vec<String>,
    /// Present, but not asked for.
    pub(super) unexpected: Vec<String>,
}

/// A short human label for a record, including the fields that tell records
/// with the same value apart (MX priority, CAA tag, TLSA usage, …).
fn describe(record: &types::DnsRecord) -> String {
    let mut parts = vec![record.type_.as_str().to_owned()];
    let numbers = [
        ("priority", record.priority.map(u32::from)),
        ("weight", record.weight.map(u32::from)),
        ("port", record.port.map(u32::from)),
        ("flag", record.flag.map(u32::from)),
        ("usage", record.usage.as_ref().map(|u| u32::from(u.0))),
        ("selector", record.selector.as_ref().map(|s| u32::from(s.0))),
        (
            "matching type",
            record.matching_type.as_ref().map(|m| u32::from(m.0)),
        ),
    ];
    for (label, number) in numbers {
        if let Some(number) = number {
            parts.push(format!("{label} {number}"));
        }
    }
    if let Some(tag) = &record.tag {
        parts.push(format!("tag {tag}"));
    }
    parts.push(record_value(record).unwrap_or("(no data)").to_owned());
    if let Some(parameters) = &record.parameters {
        parts.push(format!("parameters {parameters}"));
    }
    parts.join(" ")
}

/// Compare the records now present against those asked for, as multisets.
/// `None` when they hold the same content. Pure so the rules are unit-testable.
pub(super) fn final_set_mismatch(
    desired: &[types::DnsRecord],
    actual: &[types::DnsRecord],
    compare_ttl: bool,
) -> Option<Mismatch> {
    let mut remaining: Vec<(serde_json::Value, &types::DnsRecord)> = actual
        .iter()
        .map(|r| (comparable(r, compare_ttl), r))
        .collect();
    let mut missing = Vec::new();
    for want in desired {
        let key = comparable(want, compare_ttl);
        match remaining.iter().position(|(have, _)| *have == key) {
            Some(i) => {
                remaining.remove(i);
            }
            None => missing.push(describe(want)),
        }
    }
    let unexpected: Vec<String> = remaining.iter().map(|(_, r)| describe(r)).collect();
    if missing.is_empty() && unexpected.is_empty() {
        None
    } else {
        Some(Mismatch {
            missing,
            unexpected,
        })
    }
}

/// Read the type+name back and check it against `desired`. `Ok` with the records
/// as they now stand when they match; otherwise a non-zero error that lists what is extra and what is
/// missing and leaves the zone as it is — a record that appeared in the
/// meantime may belong to someone else, so it is reported rather than removed.
pub(super) async fn verify_final_state(
    client: &domains_client::Client,
    domain: &str,
    record_type: &str,
    name: &str,
    desired: &[types::DnsRecord],
    compare_ttl: bool,
    debug: bool,
) -> Result<Vec<types::DnsRecord>, CliCoreError> {
    let review = format!("gddy dns list {domain} --type {record_type} --name {name}");
    let actual = fetch_records(client, domain, Some(record_type), Some(name), debug)
        .await
        .map_err(|e| {
            crate::error::GddyError::unexpected(format!(
                "the change was applied, but reading {name} ({record_type}) back \
                to confirm it failed: {e}"
            ))
            .with_fix(format!("Run `{review}` to check the current records."))
            .into_cli_error()
        })?;

    let Some(mismatch) = final_set_mismatch(desired, &actual, compare_ttl) else {
        return Ok(actual);
    };
    let list = |items: &[String]| {
        items
            .iter()
            .map(|item| format!("  - {item}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let mut message = format!(
        "set finished, but the records now at {name} ({record_type}) are not the \
        ones you asked for. Someone else may have changed them while this ran."
    );
    if !mismatch.unexpected.is_empty() {
        message.push_str(&format!(
            "\nPresent but not requested:\n{}",
            list(&mismatch.unexpected)
        ));
    }
    if !mismatch.missing.is_empty() {
        message.push_str(&format!(
            "\nRequested but not present:\n{}",
            list(&mismatch.missing)
        ));
    }
    Err(crate::error::GddyError::unexpected(message)
        .with_fix(format!(
            "Nothing extra was removed. Run `{review}` to review the records, \
            then re-run `set` to replace them or `gddy dns delete` to remove the \
            unexpected ones."
        ))
        .into_cli_error())
}

#[cfg(test)]
#[path = "verify_tests.rs"]
mod tests;
