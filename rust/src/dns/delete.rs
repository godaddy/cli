//! `dns delete` — remove all records matching a type+name from a domain.

use cli_engine::{Alignment, CommandResult, CommandSpec, RuntimeCommandSpec, TableColumn, Tier};
use serde_json::{Value, json};

use crate::domain::make_client;
use crate::idn::DomainFormArgs;
use crate::output_schema::output_schema;
use crate::scopes::DOMAINS_DNS_UPDATE;

use domains_client::types;

use super::failure::{DnsFailure, MutateFailure};
use super::records::{
    ZONE_AND_NAME, change_columns, change_entry, fetch_records, parse_write_type_arg,
    record_label_value, verified_result,
};

output_schema!(DnsDeleteResult {
    "domain": "string";
    "type": "string";
    "name": "string";
    "deleted": "number";
    "failed": "number";
    "action": "string";
    "records": "[]object";
});

/// The `action` of a `delete` that matched nothing, in a preview and a real run alike.
const NOTHING_TO_DELETE: &str = "nothing to delete";

/// One matched record's delete, as reported: the record, and `None` on success or
/// `Some(error)` if it could not be removed.
type DeleteOutcome = (types::DnsRecord, Option<DnsFailure>);

/// Why a matched record that came back without a recordId can't be deleted. The
/// real run reports it as a failure and the preview as a `fail` entry, in these
/// same words.
fn missing_record_id_reason(domain: &str, record_type: &str, name: &str) -> String {
    format!(
        "the API returned this record without a recordId, so it can't be deleted; re-run \
         `gddy dns list {domain} --type {record_type} --name {name}` and remove it in the \
         control panel if it persists"
    )
}

/// The `delete` result for the matched records — the same builder for the
/// preview and the real run, so the two cannot drift. `records` lists each record
/// with its id and value; `deleted`/`failed` count them; a record without a
/// recordId can't be targeted and counts as failed.
fn delete_result(
    domain: &str,
    record_type: &str,
    name: &str,
    deleted: usize,
    failed: usize,
    action: &str,
    records: Vec<Value>,
) -> Value {
    json!({
        "domain": domain,
        "type": record_type,
        "name": name,
        "deleted": deleted,
        "failed": failed,
        "action": if records.is_empty() { NOTHING_TO_DELETE } else { action },
        "records": records,
    })
}

/// Build the `delete` result from per-record delete outcomes: `Ok(json)` with the
/// deleted count (0 when nothing matched) and the removed `records` if all
/// succeeded, else `Err(message)` — a non-zero exit — with a per-record ✓/✗
/// breakdown. Pure so it's unit-testable.
fn summarize_delete_outcomes(
    domain: &str,
    record_type: &str,
    name: &str,
    outcomes: &[DeleteOutcome],
) -> Result<Value, MutateFailure> {
    let label =
        |rec: &types::DnsRecord| record_label_value(rec).unwrap_or_else(|| "(no data)".to_owned());
    let failed = outcomes.iter().filter(|(_, e)| e.is_some()).count();
    let deleted = outcomes.len() - failed;

    if failed > 0 {
        let breakdown = outcomes
            .iter()
            .map(|(rec, err)| match err {
                None => format!("  ✓ {} — deleted", label(rec)),
                Some(e) => format!("  ✗ {} — {e}", label(rec)),
            })
            .collect::<Vec<_>>()
            .join("\n");
        return Err(MutateFailure::new(
            format!(
                "deleted {deleted} of {} record(s) for {name} ({record_type}); \
                {failed} failed:\n{breakdown}\n\nRe-run \
                `gddy dns delete {domain} --type {record_type} --name {name}`, \
                or `gddy dns list {domain} --type {record_type} --name {name}` \
                to review the current state.",
                outcomes.len(),
            ),
            outcomes.iter().filter_map(|(_, e)| e.as_ref()),
        ));
    }

    let records = outcomes
        .iter()
        .map(|(rec, _)| change_entry("delete", rec.record_id.as_deref(), Some(rec), None))
        .collect();
    Ok(delete_result(
        domain,
        record_type,
        name,
        deleted,
        failed,
        "delete",
        records,
    ))
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
    let records = existing
        .iter()
        .map(|rec| match rec.record_id.as_deref() {
            Some(id) => change_entry("delete", Some(id), Some(rec), None),
            None => {
                let mut entry = change_entry("fail", None, Some(rec), None);
                entry["error"] = json!(missing_record_id_reason(domain, record_type, name));
                entry
            }
        })
        .collect();
    delete_result(
        domain,
        record_type,
        name,
        would_delete,
        existing.len() - would_delete,
        "would delete",
        records,
    )
}

#[derive(Debug, Clone, clap::Args)]
struct DeleteArgs {
    /// Domain whose records to delete (e.g. example.com).
    #[arg(value_name = "DOMAIN", value_parser = crate::idn::parse_name_arg)]
    domain: String,

    /// Record type (A, AAAA, ALIAS, CAA, CNAME, HTTPS, MX, SRV, SVCB, TLSA, TXT).
    #[arg(long = "type", value_name = "TYPE", value_parser = parse_write_type_arg)]
    record_type: String,

    /// Record name relative to the domain (e.g. www).
    #[arg(long, value_name = "NAME", value_parser = crate::idn::parse_name_arg)]
    name: String,

    #[command(flatten)]
    domain_format: DomainFormArgs,
}

/// `records` lists the matched records, each with its id and what it held — in a
/// preview and in the real delete — as an indented child table instead of a raw
/// JSON dump.
fn view_columns() -> Vec<TableColumn> {
    vec![
        TableColumn::new("domain", "Domain"),
        TableColumn::new("type", "Type"),
        TableColumn::new("name", "Name"),
        TableColumn::new("deleted", "Deleted").align(Alignment::Right),
        TableColumn::new("failed", "Failed").align(Alignment::Right),
        TableColumn::new("action", "Action"),
        TableColumn::new("records", "Records")
            .hide_when_empty(true)
            .nested(change_columns()),
    ]
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<DeleteArgs, _, _, _>(
        CommandSpec::from_args::<DeleteArgs>(
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
        .with_default_fields("domain,type,name,deleted,failed,action,records")
        .with_output_schema::<DnsDeleteResult>()
        .with_view(view_columns())
        .with_scopes(&[DOMAINS_DNS_UPDATE]),
        |ctx, args: DeleteArgs| async move {
            let domain = args.domain;
            let record_type = args.record_type;
            let name = args.name;
            let domain_format = args.domain_format.form(&ctx)?;

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
                let mut preview = dry_run_delete_preview(&domain, &record_type, &name, &existing);
                domain_format.convert_fields(&mut preview, ZONE_AND_NAME);
                return Ok(CommandResult::new(preview).with_dry_run());
            }

            let mut outcomes: Vec<DeleteOutcome> = Vec::with_capacity(existing.len());
            for rec in existing {
                // A record with no server id can't be targeted — report it as a
                // failure (non-zero exit) rather than silently leaving it behind.
                let err = match rec.record_id.as_deref() {
                    None => Some(DnsFailure::from(missing_record_id_reason(
                        &domain,
                        &record_type,
                        &name,
                    ))),
                    Some(id) => match client
                        .delete_dns_record()
                        .zone(domain.as_str())
                        .record_id(id)
                        .send()
                        .await
                    {
                        Ok(_) => None,
                        Err(e) => {
                            Some(DnsFailure::from_error("deleting DNS record", debug, e).await)
                        }
                    },
                };
                outcomes.push((rec, err));
            }

            summarize_delete_outcomes(&domain, &record_type, &name, &outcomes)
                .map(|v| verified_result(domain_format, v, &domain, &record_type, &name))
                .map_err(super::mutate_failed)
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summarize_delete_reports_count_zero_and_partial_failure() {
        // Nothing matched → deleted 0, not an error.
        let v = summarize_delete_outcomes("example.com", "A", "www", &[]).expect("ok");
        assert_eq!(v["deleted"], 0);
        assert_eq!(v["action"], "nothing to delete");
        // All deleted.
        let ok = vec![
            (test_record("r1", "1.2.3.4"), None),
            (test_record("r2", "5.6.7.8"), None),
        ];
        let v = summarize_delete_outcomes("example.com", "A", "www", &ok).expect("ok");
        assert_eq!(v["deleted"], 2);
        assert_eq!(v["action"], "delete");
        // One failed → non-zero error naming the failed value.
        let mixed = vec![
            (test_record("r1", "1.2.3.4"), None),
            (
                test_record("r2", "5.6.7.8"),
                Some("nope".to_string().into()),
            ),
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

    /// The point of one shared shape: the same delete, previewed or done, lists the
    /// same records with the same ids, old values and actions — a script reads
    /// both the same way, and a real run says what it removed.
    #[test]
    fn a_real_delete_reports_the_same_records_as_its_preview() {
        let existing = vec![test_record("r1", "1.2.3.4"), test_record("r2", "5.6.7.8")];
        let preview = dry_run_delete_preview("example.com", "A", "www", &existing);
        let outcomes: Vec<DeleteOutcome> = existing.iter().cloned().map(|r| (r, None)).collect();
        let done = summarize_delete_outcomes("example.com", "A", "www", &outcomes).expect("ok");

        assert_eq!(preview["records"], done["records"]);
        assert_eq!(done["records"][0]["recordId"], "r1");
        assert_eq!(done["records"][0]["old"]["data"], "1.2.3.4");
        assert_eq!(done["records"][0]["new"], Value::Null);
        assert_eq!(done["records"][1]["recordId"], "r2");
    }

    /// Nothing to delete reads the same in a preview and a real run.
    #[test]
    fn nothing_to_delete_is_worded_the_same_in_a_preview_and_a_real_run() {
        let preview = dry_run_delete_preview("example.com", "A", "www", &[]);
        let done = summarize_delete_outcomes("example.com", "A", "www", &[]).expect("ok");
        assert_eq!(preview["action"], "nothing to delete");
        assert_eq!(done["action"], preview["action"]);
        assert_eq!(done["deleted"], preview["deleted"]);
    }

    fn test_record(record_id: &str, data: &str) -> types::DnsRecord {
        types::DnsRecord {
            certificate_data: None,
            matching_type: None,
            selector: None,
            usage: None,
            data: Some(data.to_owned()),
            flag: None,
            name: "www".to_owned(),
            parameters: None,
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

    /// A TLSA record as `v3_record` actually builds one: its value lives in
    /// `certificate_data`, not `data`.
    fn test_tlsa_record(record_id: &str, cert: &str) -> types::DnsRecord {
        types::DnsRecord {
            certificate_data: Some(cert.to_owned()),
            data: None,
            type_: types::DnsRecordType("TLSA".to_owned()),
            ..test_record(record_id, "")
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
        assert_eq!(records[0]["old"]["data"], "1.2.3.4");
        assert_eq!(records[0]["recordId"], "r1");
        assert_eq!(records[0]["action"], "delete");
    }

    /// TLSA's value lives in `certificate_data`, not `data` — the preview
    /// must read through `record_value` to show it rather than reporting
    /// null.
    #[test]
    fn dry_run_delete_preview_shows_tlsa_certificate_data_as_the_value() {
        let cert = "d2abde240d7cd3ee6b4b28c54df034b97983a1d16e8a410e4561cb106618e971";
        // No numeric fields: it falls back to the certificate alone, not null.
        let existing = vec![test_tlsa_record("r1", cert)];
        let preview = dry_run_delete_preview("example.com", "TLSA", "www", &existing);
        let records = preview["records"].as_array().expect("records array");
        assert_eq!(records[0]["old"]["data"], cert);
    }

    /// Two TLSA records with the same certificate, port and protocol that differ
    /// only in usage are different records, and `delete --type TLSA --name www`
    /// removes both: the preview has to show which is which.
    #[test]
    fn dry_run_delete_preview_tells_tlsa_records_apart_by_usage_selector_and_matching_type() {
        let cert = "d2abde240d7cd3ee6b4b28c54df034b97983a1d16e8a410e4561cb106618e971";
        let tlsa = |id: &str, usage: u8, selector: u8, matching: u8| types::DnsRecord {
            usage: Some(types::TlsaUsage(usage)),
            selector: Some(types::TlsaSelector(selector)),
            matching_type: Some(types::TlsaMatchingType(matching)),
            port: Some(443),
            protocol: Some("_tcp".to_owned()),
            ..test_tlsa_record(id, cert)
        };
        let existing = vec![tlsa("r1", 3, 1, 1), tlsa("r2", 1, 0, 2)];

        let preview = dry_run_delete_preview("example.com", "TLSA", "www", &existing);

        let records = preview["records"].as_array().expect("records array");
        assert_eq!(records[0]["old"]["data"], format!("3 1 1 {cert}"));
        assert_eq!(records[1]["old"]["data"], format!("1 0 2 {cert}"));
        assert_ne!(records[0]["old"]["data"], records[1]["old"]["data"]);
    }

    #[test]
    fn a_record_that_is_not_tlsa_is_labelled_by_its_plain_value() {
        let a = test_record("r1", "1.2.3.4");
        assert_eq!(record_label_value(&a).as_deref(), Some("1.2.3.4"));
        let none = types::DnsRecord {
            data: None,
            ..test_record("r2", "")
        };
        assert_eq!(record_label_value(&none), None);
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
        assert_eq!(records[0]["action"], "delete");
        assert_eq!(records[1]["action"], "fail");
        assert_eq!(records[1]["recordId"], Value::Null);
        assert_eq!(
            records[1]["error"],
            missing_record_id_reason("example.com", "A", "www")
        );
    }

    /// Nothing matched — "would delete" would misleadingly imply a delete
    /// with no target; the preview should say so plainly instead.
    #[test]
    fn dry_run_delete_preview_reports_nothing_to_delete_on_zero_match() {
        let preview = dry_run_delete_preview("example.com", "A", "www", &[]);
        assert_eq!(preview["deleted"], 0);
        assert_eq!(preview["failed"], 0);
        assert_eq!(preview["action"], "nothing to delete");
        assert_eq!(
            preview["records"].as_array().expect("records array").len(),
            0
        );
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
        let default_fields = "domain,type,name,deleted,failed,action,records";
        let projected = cli_engine::output::filter_fields(&preview, default_fields);
        for field in [
            "domain", "type", "name", "deleted", "failed", "action", "records",
        ] {
            assert!(
                !projected[field].is_null(),
                "{field:?} was stripped by default_fields; preview: {projected}"
            );
        }
    }

    fn test_srv_record(record_id: &str, service: &str, port: u16) -> types::DnsRecord {
        types::DnsRecord {
            data: Some("chat-server.example.com.".to_owned()),
            name: "chat".to_owned(),
            port: Some(port),
            priority: Some(10),
            weight: Some(5),
            protocol: Some("_tcp".to_owned()),
            service: Some(service.to_owned()),
            type_: types::DnsRecordType("SRV".to_owned()),
            ..test_record(record_id, "")
        }
    }

    /// Deleting `--type SRV --name chat` removes every service under it, so the
    /// preview has to say which services those are: otherwise the rows are
    /// identical and the customer cannot see what is about to go.
    #[test]
    fn dry_run_delete_preview_tells_srv_records_apart() {
        let existing = vec![
            test_srv_record("r1", "_sip", 5060),
            test_srv_record("r2", "_xmpp", 5222),
        ];
        let preview = dry_run_delete_preview("example.com", "SRV", "chat", &existing);

        let records = preview["records"].as_array().expect("records array");
        assert_eq!(records[0]["old"]["service"], "_sip");
        assert_eq!(records[1]["old"]["port"], 5222);

        let rendered = cli_engine::preview_human_view(preview, &view_columns());
        assert!(
            rendered.contains("SERVICE") && rendered.contains("PORT"),
            "{rendered}"
        );
        assert!(
            rendered.contains("_sip") && rendered.contains("_xmpp"),
            "{rendered}"
        );
    }

    #[test]
    fn dry_run_delete_preview_adds_no_columns_for_plain_records() {
        let existing = vec![test_record("r1", "1.2.3.4"), test_record("r2", "5.6.7.8")];
        let preview = dry_run_delete_preview("example.com", "A", "www", &existing);

        let rendered = cli_engine::preview_human_view(preview, &view_columns());

        for sparse in [
            "SERVICE", "PORT", "PRIORITY", "PROTOCOL", "WEIGHT", "TAG", "FLAG",
        ] {
            assert!(!rendered.contains(sparse), "{sparse} shown: {rendered}");
        }
        assert!(
            rendered.contains("1.2.3.4") && rendered.contains("would delete"),
            "{rendered}"
        );
    }

    /// Proves `view_columns()`'s field names actually match what
    /// `dry_run_delete_preview` emits — a mismatch here would silently drop
    /// the record breakdown from human output the same way the `api
    /// describe` view once dropped its ambiguous-match fields.
    #[test]
    fn dry_run_delete_preview_renders_the_records_as_a_nested_table() {
        let existing = vec![test_record("r1", "1.2.3.4")];
        let preview = dry_run_delete_preview("example.com", "A", "www", &existing);
        let rendered = cli_engine::preview_human_view(preview, &view_columns());
        assert!(rendered.contains("Records:"), "{rendered}");
        assert!(rendered.contains("RECORD ID"), "{rendered}");
        assert!(rendered.contains("r1"), "{rendered}");
        assert!(rendered.contains("1.2.3.4"), "{rendered}");
        assert!(rendered.contains("would delete"), "{rendered}");
    }
}
