//! `dns delete` — remove all records matching a type+name from a domain.

use cli_engine::{CliCoreError, CommandResult, CommandSpec, RuntimeCommandSpec, Tier};
use serde_json::{Value, json};

use crate::domain::{api_error, make_client};
use crate::output_schema::output_schema;
use crate::scopes::DOMAINS_DNS_UPDATE;

use domains_client::types;

use super::records::{arg_str, fetch_records, parse_write_type_arg, verify_with_list_action};

output_schema!(DnsDeleteResult {
    "domain": "string";
    "type": "string";
    "name": "string";
    "deleted": "number";
    "failed": "number";
    "action": "string";
});

/// Build the `delete` result from per-record delete outcomes (each matched
/// record's `data` value paired with `None` on success or `Some(error)`):
/// `Ok(json)` with the deleted count (0 when nothing matched) if all succeeded,
/// else `Err(message)` — a non-zero exit — with a per-record ✓/✗ breakdown. Pure
/// so it's unit-testable.
fn summarize_delete_outcomes(
    domain: &str,
    record_type: &str,
    name: &str,
    outcomes: &[(String, Option<String>)],
) -> Result<Value, String> {
    let failed = outcomes.iter().filter(|(_, e)| e.is_some()).count();
    let deleted = outcomes.len() - failed;

    if failed > 0 {
        let breakdown = outcomes
            .iter()
            .map(|(value, err)| match err {
                None => format!("  ✓ {value} — deleted"),
                Some(e) => format!("  ✗ {value} — {e}"),
            })
            .collect::<Vec<_>>()
            .join("\n");
        return Err(format!(
            "deleted {deleted} of {} record(s) for {name} ({record_type}); {failed} failed:\n\
             {breakdown}\n\nRe-run `gddy dns delete {domain} --type {record_type} --name \
             {name}`, or `gddy dns list {domain} --type {record_type} --name {name}` to \
             review the current state.",
            outcomes.len(),
        ));
    }

    Ok(json!({
        "domain": domain,
        "type": record_type,
        "name": name,
        "deleted": deleted,
        "failed": failed,
        "action": "delete",
    }))
}

/// Builds the `dns delete --dry-run` preview from the same matched records the
/// real execution would delete, so the preview can never drift from what
/// `delete` would actually do.
fn dry_run_delete_preview(
    domain: &str,
    record_type: &str,
    name: &str,
    existing: &[types::DnsRecord],
) -> Value {
    // Mirror the real execution's per-record split: a record without a
    // recordId can't actually be deleted (see the handler below), so the
    // preview must not claim it as a delete.
    let would_delete = existing.iter().filter(|r| r.record_id.is_some()).count();
    let would_fail = existing.len() - would_delete;
    let records: Vec<Value> = existing
        .iter()
        .map(|rec| {
            let status = if rec.record_id.is_some() {
                "would delete"
            } else {
                "would fail (missing recordId)"
            };
            json!({"recordId": rec.record_id, "data": rec.data, "status": status})
        })
        .collect();

    json!({
        "domain": domain,
        "type": record_type,
        "name": name,
        // Reuse DnsDeleteResult's own field names (rather than inventing
        // "wouldDelete"/"wouldFail") so the preview survives the command's
        // `with_default_fields` projection instead of being silently
        // stripped down to just domain/type/name.
        "deleted": would_delete,
        "failed": would_fail,
        "records": records,
        "action": "would delete",
    })
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_with_context(
        CommandSpec::new(
            "delete",
            "Delete all records for a type+name (destructive: removes existing)",
        )
        .with_long(
            "Removes every DNS record matching the given type+name pair. v3 deletes \
             one record at a time, so this lists the matching records and deletes \
             each; a partial failure is reported per record. This is destructive \
             and irreversible. NS and SOA records are GoDaddy-managed and cannot be \
             deleted. Use `dns list` to confirm what will be removed first.",
        )
        .with_system("domain")
        .with_tier(Tier::Destructive)
        .handles_dry_run(true)
        .with_default_fields("domain,type,name,deleted,failed,action")
        .with_output_schema::<DnsDeleteResult>()
        .with_scopes(&[DOMAINS_DNS_UPDATE])
        .with_arg(
            clap::Arg::new("domain")
                .value_name("DOMAIN")
                .required(true)
                .help("Domain whose records to delete (e.g. example.com)"),
        )
        .with_arg(
            clap::Arg::new("type")
                .long("type")
                .value_name("TYPE")
                .required(true)
                .value_parser(parse_write_type_arg)
                .help("Record type (A, AAAA, ALIAS, CAA, CNAME, MX, SRV, TXT)"),
        )
        .with_arg(
            clap::Arg::new("name")
                .long("name")
                .value_name("NAME")
                .required(true)
                .help("Record name relative to the domain (e.g. www)"),
        ),
        |ctx| async move {
            let domain = arg_str(&ctx, "domain").unwrap_or_default();
            let record_type = arg_str(&ctx, "type").unwrap_or_default();
            let name = arg_str(&ctx, "name").unwrap_or_default();

            let debug = !ctx.middleware.debug.is_empty();
            let client = make_client(&ctx).await?;

            // v3 deletes by record id, so find the matching records first.
            let existing = fetch_records(
                &client,
                domain.as_str(),
                Some(&record_type),
                Some(&name),
                debug,
            )
            .await?;

            if ctx.dry_run() {
                return Ok(CommandResult::new(dry_run_delete_preview(
                    &domain,
                    &record_type,
                    &name,
                    &existing,
                ))
                .with_dry_run());
            }

            let mut outcomes = Vec::with_capacity(existing.len());
            for rec in &existing {
                // A record with no server id can't be targeted — report it as a
                // failure (non-zero exit) rather than silently leaving it behind.
                let err = match rec.record_id.as_deref() {
                    None => Some(format!(
                        "the API returned this record without a recordId, so it can't be \
                         deleted; re-run `gddy dns list {domain} --type {record_type} --name \
                         {name}` and remove it in the control panel if it persists"
                    )),
                    Some(id) => match client
                        .delete_dns_record()
                        .zone(domain.as_str())
                        .record_id(id)
                        .send()
                        .await
                    {
                        Ok(_) => None,
                        Err(e) => {
                            Some(api_error("deleting DNS record", debug, e).await.to_string())
                        }
                    },
                };
                outcomes.push((rec.data.clone(), err));
            }

            summarize_delete_outcomes(&domain, &record_type, &name, &outcomes)
                .map(|v| {
                    CommandResult::new(v).with_next_actions(vec![verify_with_list_action(
                        &domain,
                        &record_type,
                        &name,
                    )])
                })
                .map_err(CliCoreError::message)
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summarize_delete_reports_count_zero_and_partial_failure() {
        // Nothing matched → deleted 0, not an error.
        let none: [(String, Option<String>); 0] = [];
        let v = summarize_delete_outcomes("example.com", "A", "www", &none).expect("ok");
        assert_eq!(v["deleted"], 0);
        // All deleted.
        let ok = vec![("1.2.3.4".to_string(), None), ("5.6.7.8".to_string(), None)];
        let v = summarize_delete_outcomes("example.com", "A", "www", &ok).expect("ok");
        assert_eq!(v["deleted"], 2);
        // One failed → non-zero error naming the failed value.
        let mixed = vec![
            ("1.2.3.4".to_string(), None),
            ("5.6.7.8".to_string(), Some("nope".to_string())),
        ];
        let err =
            summarize_delete_outcomes("example.com", "A", "www", &mixed).expect_err("a failure");
        assert!(err.contains("deleted 1 of 2"), "{err}");
        assert!(err.contains("✗ 5.6.7.8 — nope"), "{err}");
        // The recovery hint must include the domain positional and flags or it won't parse.
        assert!(
            err.contains("dns delete example.com --type A --name www"),
            "{err}"
        );
        assert!(
            err.contains("dns list example.com --type A --name www"),
            "{err}"
        );
    }

    fn test_record(record_id: &str, data: &str) -> types::DnsRecord {
        types::DnsRecord {
            data: data.to_owned(),
            flag: None,
            name: "www".to_owned(),
            port: None,
            priority: None,
            protocol: None,
            record_id: Some(record_id.to_owned()),
            service: None,
            tag: None,
            ttl: 3600,
            type_: types::DnsRecordType("A".to_owned()),
            weight: None,
        }
    }

    fn test_record_without_id(data: &str) -> types::DnsRecord {
        types::DnsRecord {
            record_id: None,
            ..test_record("unused", data)
        }
    }

    #[test]
    fn dry_run_delete_preview_lists_every_matched_record_without_deleting() {
        let existing = vec![test_record("r1", "1.2.3.4"), test_record("r2", "5.6.7.8")];
        let preview = dry_run_delete_preview("example.com", "A", "www", &existing);
        assert_eq!(preview["deleted"], 2);
        assert_eq!(preview["failed"], 0);
        assert_eq!(preview["action"], "would delete");
        let records = preview["records"].as_array().expect("records array");
        assert_eq!(records.len(), 2);
        assert_eq!(records[0]["data"], "1.2.3.4");
        assert_eq!(records[0]["status"], "would delete");
    }

    /// A record without a recordId can't actually be deleted (the real
    /// handler reports it as a failure) — the preview must not claim it as
    /// a delete either.
    #[test]
    fn dry_run_delete_preview_flags_records_without_a_recordid_as_would_fail() {
        let existing = vec![
            test_record("r1", "1.2.3.4"),
            test_record_without_id("5.6.7.8"),
        ];
        let preview = dry_run_delete_preview("example.com", "A", "www", &existing);
        assert_eq!(preview["deleted"], 1);
        assert_eq!(preview["failed"], 1);
        let records = preview["records"].as_array().expect("records array");
        assert_eq!(records[1]["status"], "would fail (missing recordId)");
    }

    /// The command's `--output json` path always projects through
    /// `default_fields` when the user doesn't pass `--fields` — a preview
    /// field that isn't in that list is silently stripped and never reaches
    /// the user. Prove the preview's summary fields actually survive it
    /// (this is exactly the gap a pure call to `dry_run_delete_preview` can't
    /// catch, since it never goes through the projection).
    #[test]
    fn dry_run_delete_preview_survives_default_field_projection() {
        let existing = vec![test_record("r1", "1.2.3.4")];
        let preview = dry_run_delete_preview("example.com", "A", "www", &existing);
        let default_fields = "domain,type,name,deleted,failed,action";
        let projected = cli_engine::output::filter_fields(&preview, default_fields);
        for field in ["domain", "type", "name", "deleted", "failed", "action"] {
            assert!(
                !projected[field].is_null(),
                "{field:?} was stripped by default_fields; preview: {projected}"
            );
        }
    }
}
