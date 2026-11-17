//! `dns add` — append new DNS records to a domain without touching existing ones.

use cli_engine::{Alignment, CommandResult, CommandSpec, RuntimeCommandSpec, TableColumn, Tier};
use domains_client::types;
use serde_json::{Value, json};

use crate::domain::make_client;
use crate::output_schema::output_schema;
use crate::scopes::DOMAINS_DNS_UPDATE;

use super::conflicts::{
    WriteErrorContext, conflicting_records, describe_duplicate_record, describe_write_error,
};
use super::records::{
    RecordOptions, RecordWriteArgs, ZONE_AND_NAME, change_columns, change_entry, fetch_records,
    same_content, v3_record, v3_records, validate_caa_fields, validate_record_name,
    validate_svcb_fields, validate_tlsa_fields, verified_result,
};

// `dns add` creates one v3 record per `--data` value; it reports each outcome
// individually so a partial failure is explicit (see the handler).
output_schema!(DnsAddResult {
    "domain": "string";
    "type": "string";
    "name": "string";
    "created": "number";
    "failed": "number";
    "action": "string";
    "records": "[]object";
});

/// What happened to one `--data` value: the record the API created, or why it
/// could not be.
type AddOutcome = (String, Result<types::DnsRecord, String>);

/// The `add` result: the counts and the `records` created (or, with
/// `--dry-run`, that would be).
fn add_result(
    domain: &str,
    record_type: &str,
    name: &str,
    created: usize,
    failed: usize,
    action: &str,
    records: Vec<Value>,
) -> Value {
    json!({
        "domain": domain,
        "type": record_type,
        "name": name,
        "created": created,
        "failed": failed,
        "action": action,
        "records": records,
    })
}

/// Summarize the per-record create outcomes of `dns add` (each `--data` value
/// paired with the record created, or an `Err(message)`), preserving input order.
/// Returns the success JSON payload when *every* record was created, or an error
/// message (a non-zero exit) with a per-record breakdown if any failed. Pure — no
/// I/O — so the aggregation and the success/failure decision are unit-testable.
fn summarize_add_outcomes(
    domain: &str,
    record_type: &str,
    name: &str,
    outcomes: Vec<AddOutcome>,
) -> Result<Value, String> {
    let failed = outcomes.iter().filter(|(_, o)| o.is_err()).count();
    let total = outcomes.len();
    let created = total - failed;

    if failed > 0 {
        let breakdown = outcomes
            .iter()
            .map(|(value, outcome)| match outcome {
                Ok(_) => format!("  ✓ {value} — created"),
                Err(err) => format!("  ✗ {value} — {err}"),
            })
            .collect::<Vec<_>>()
            .join("\n");
        return Err(format!(
            "added {created} of {total} DNS record(s) for {name} ({record_type}); {failed} \
             failed:\n{breakdown}\n\nRe-run `gddy dns add {domain} --type {record_type} --name \
             {name} --data <value>` with just the failed value(s), or `gddy dns list {domain} \
             --type {record_type} --name {name}` to review the current state."
        ));
    }

    let records = outcomes
        .iter()
        .filter_map(|(_, outcome)| outcome.as_ref().ok())
        .map(|record| change_entry("create", record.record_id.as_deref(), None, Some(record)))
        .collect();
    Ok(add_result(
        domain,
        record_type,
        name,
        created,
        failed,
        "add",
        records,
    ))
}

/// Why `record` could not be added next to `existing_at_name`, as the API would
/// refuse it — an exact duplicate, or a CNAME against other types — or `None` when
/// nothing in the zone stands in its way. The API's own checks (value syntax, say)
/// are not repeated here; a preview can only promise what it can see.
fn add_blocker(
    domain: &str,
    name: &str,
    record_type: &str,
    data: &str,
    opts: &RecordOptions,
    existing_at_name: &[types::DnsRecord],
) -> Option<String> {
    let record = v3_record(name, record_type, data, opts);
    let duplicate = existing_at_name
        .iter()
        .any(|r| r.type_.as_str() == record_type && same_content(r, &record));
    if duplicate || !conflicting_records(record_type, existing_at_name).is_empty() {
        Some(describe_duplicate_record(
            record_type,
            data,
            domain,
            name,
            opts,
            existing_at_name,
            false,
        ))
    } else {
        None
    }
}

/// The `dns add --dry-run` preview: one `create` per `--data` value, each with the
/// record it would write, or a `fail` entry carrying the reason when something
/// already in the zone would make the API refuse it. The `new` record has no
/// recordId yet — the API assigns it — so a real run's result differs only there.
fn dry_run_add_preview(
    domain: &str,
    record_type: &str,
    name: &str,
    data: &[String],
    opts: &RecordOptions,
    existing_at_name: &[types::DnsRecord],
) -> Value {
    let mut failed = 0;
    let records: Vec<Value> = v3_records(name, record_type, data, opts)
        .iter()
        .zip(data)
        .map(|(record, value)| {
            let mut entry = change_entry("create", None, None, Some(record));
            if let Some(reason) =
                add_blocker(domain, name, record_type, value, opts, existing_at_name)
            {
                failed += 1;
                entry["action"] = json!("fail");
                entry["error"] = json!(reason);
            }
            entry
        })
        .collect();
    add_result(
        domain,
        record_type,
        name,
        data.len() - failed,
        failed,
        "would add",
        records,
    )
}

/// `records` renders as an indented child table instead
/// of a raw JSON dump.
fn view_columns() -> Vec<TableColumn> {
    vec![
        TableColumn::new("domain", "Domain").essential(true),
        TableColumn::new("type", "Type"),
        TableColumn::new("name", "Name"),
        TableColumn::new("created", "Created").align(Alignment::Right),
        TableColumn::new("failed", "Failed").align(Alignment::Right),
        TableColumn::new("action", "Action"),
        TableColumn::new("records", "Records")
            .hide_when_empty(true)
            .nested(change_columns()),
    ]
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<RecordWriteArgs, _, _, _>(
        CommandSpec::from_args::<RecordWriteArgs>(
            "add",
            "Add DNS records to a domain (appends; non-destructive)",
        )
        .with_long(
            "Appends one or more DNS records to a domain without modifying any \
            existing records. Pass `--data` once per record value to add \
            multiple records for the same type+name. `--ttl` is 600 to 86400 \
            seconds and defaults to 3600. DNS only allows a CNAME or other \
            record types at a name. Pass `--dry-run` to see the records that \
            would be created without actually writing them.",
        )
        .with_system("domain")
        .with_tier(Tier::Mutate)
        .handles_dry_run(true)
        .with_default_fields("domain,type,name,created,failed,action,records")
        .with_output_schema::<DnsAddResult>()
        .with_view(view_columns())
        .with_scopes(&[DOMAINS_DNS_UPDATE]),
        |ctx, args: RecordWriteArgs| async move {
            let opts = RecordOptions::from_write_args(&args);
            let domain = args.domain;
            let record_type = args.record_type;
            let name = args.name;
            let domain_format = args.domain_format.form(&ctx)?;
            validate_record_name(&record_type, &name)
                .map_err(crate::error::GddyError::validation)?;
            validate_caa_fields(&record_type, &opts)
                .map_err(crate::error::GddyError::validation)?;
            validate_tlsa_fields(&record_type, &opts)
                .map_err(crate::error::GddyError::validation)?;
            validate_svcb_fields(&record_type, &opts)
                .map_err(crate::error::GddyError::validation)?;
            let data = args.data;
            let records = v3_records(&name, &record_type, &data, &opts);

            let debug = !ctx.middleware.debug.is_empty();
            let client = make_client(&ctx).await?;

            if ctx.dry_run() {
                // Every type at the name, not just this one: a CNAME and other
                // types can't share it.
                let at_name =
                    fetch_records(&client, domain.as_str(), None, Some(&name), debug).await?;
                let mut preview =
                    dry_run_add_preview(&domain, &record_type, &name, &data, &opts, &at_name);
                domain_format.convert_fields(&mut preview, ZONE_AND_NAME);
                return Ok(CommandResult::new(preview).with_dry_run());
            }

            // v3 creates a single record per call. Attempt every record —
            // don't stop at the first failure — and record each outcome, so a
            // partial failure is explicit rather than leaving the user unsure
            // which of the records were actually created. `data` and `records`
            // are parallel (one record per `--data` value).
            let mut outcomes = Vec::with_capacity(records.len());
            for (value, record) in data.iter().zip(records) {
                let outcome = match client
                    .create_dns_record()
                    .zone(domain.as_str())
                    .body(record)
                    .send()
                    .await
                {
                    Ok(created) => Ok(created.into_inner()),
                    Err(e) => Err(describe_write_error(
                        &client,
                        &WriteErrorContext {
                            domain: domain.as_str(),
                            name: name.as_str(),
                            desired_type: record_type.as_str(),
                            desired_data: value.as_str(),
                            opts: &opts,
                            action: "adding DNS record",
                            debug,
                        },
                        e,
                    )
                    .await),
                };
                outcomes.push((value.clone(), outcome));
            }

            // All-created → success payload; any failure → non-zero error
            // with a per-record breakdown.
            summarize_add_outcomes(&domain, &record_type, &name, outcomes)
                .map(|v| verified_result(domain_format, v, &domain, &record_type, &name))
                .map_err(super::mutate_failed)
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts() -> RecordOptions {
        RecordOptions {
            ttl: None,
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

    /// A record as the API returns it after a create: the built record plus its id.
    fn created(id: &str, ty: &str, name: &str, value: &str, opts: &RecordOptions) -> AddOutcome {
        (
            value.to_string(),
            Ok(types::DnsRecord {
                record_id: Some(id.to_owned()),
                ..v3_record(name, ty, value, opts)
            }),
        )
    }

    #[test]
    fn dns_add_all_created_reports_each_record_with_its_new_id() {
        let o = opts();
        let payload = summarize_add_outcomes(
            "example.com",
            "A",
            "www",
            vec![
                created("r1", "A", "www", "1.2.3.4", &o),
                created("r2", "A", "www", "5.6.7.8", &o),
            ],
        )
        .expect("all created -> success payload");
        assert_eq!(payload["created"], 2);
        assert_eq!(payload["failed"], 0);
        assert_eq!(payload["action"], "add");
        let records = payload["records"].as_array().expect("records array");
        assert_eq!(records.len(), 2);
        assert_eq!(records[0]["action"], "create");
        assert_eq!(records[0]["recordId"], "r1");
        assert_eq!(records[0]["old"], Value::Null);
        assert_eq!(records[0]["new"]["data"], "1.2.3.4");
        assert_eq!(records[1]["recordId"], "r2");
        assert_eq!(records[1]["new"]["data"], "5.6.7.8");
    }

    /// A real add reports what its preview promised, apart from the ids the API
    /// assigns on creation.
    #[test]
    fn a_real_add_reports_the_same_records_as_its_preview_apart_from_the_new_ids() {
        let o = opts();
        let data = vec!["1.2.3.4".to_string(), "5.6.7.8".to_string()];
        let preview = dry_run_add_preview("example.com", "A", "www", &data, &o, &[]);
        let done = summarize_add_outcomes(
            "example.com",
            "A",
            "www",
            vec![
                created("r1", "A", "www", "1.2.3.4", &o),
                created("r2", "A", "www", "5.6.7.8", &o),
            ],
        )
        .expect("created");

        let strip_ids = |records: &Value| -> Vec<Value> {
            records
                .as_array()
                .expect("records array")
                .iter()
                .map(|entry| json!({ "action": entry["action"], "new": entry["new"] }))
                .collect()
        };
        assert_eq!(strip_ids(&preview["records"]), strip_ids(&done["records"]));
        assert_eq!(preview["records"][0]["recordId"], Value::Null);
        assert_eq!(preview["created"], done["created"]);
        assert_eq!(preview["failed"], done["failed"]);
    }

    /// Proves `view_columns()`'s field names actually match what
    /// `summarize_add_outcomes` emits — a mismatch here would silently drop
    /// the per-record breakdown from human output.
    #[test]
    fn dns_add_records_render_as_a_nested_table() {
        let payload = summarize_add_outcomes(
            "example.com",
            "A",
            "www",
            vec![created("r1", "A", "www", "1.2.3.4", &opts())],
        )
        .expect("all created -> success payload");
        let rendered = cli_engine::preview_human_view(payload, &view_columns());
        assert!(rendered.contains("Records:"), "{rendered}");
        assert!(rendered.contains("1.2.3.4"), "{rendered}");
        assert!(rendered.contains("create"), "{rendered}");
        assert!(rendered.contains("r1"), "{rendered}");
    }

    #[test]
    fn dns_add_partial_failure_is_an_error_with_per_record_breakdown() {
        // Middle record fails: the command must NOT succeed, and the message must
        // make clear which values were created and which failed (and why).
        let o = opts();
        let err = summarize_add_outcomes(
            "example.com",
            "A",
            "www",
            vec![
                created("r1", "A", "www", "1.2.3.4", &o),
                ("5.6.7.8".to_string(), Err("422 invalid data".to_string())),
                created("r3", "A", "www", "9.9.9.9", &o),
            ],
        )
        .expect_err("any failure -> error");
        assert!(err.contains("added 2 of 3"), "{err}");
        assert!(err.contains("1 failed"), "{err}");
        // Per-record breakdown names both the created and the failed values.
        assert!(err.contains("✓ 1.2.3.4"), "{err}");
        assert!(err.contains("✗ 5.6.7.8 — 422 invalid data"), "{err}");
        assert!(err.contains("✓ 9.9.9.9"), "{err}");
        // The recovery hint must include the domain positional or it won't parse.
        assert!(
            err.contains("dns add example.com --type A --name www --data <value>"),
            "{err}"
        );
        assert!(
            err.contains("dns list example.com --type A --name www"),
            "{err}"
        );
    }

    fn stored(id: &str, ty: &str, name: &str, value: &str) -> types::DnsRecord {
        types::DnsRecord {
            record_id: Some(id.to_owned()),
            ..v3_record(name, ty, value, &opts())
        }
    }

    /// A preview says what a real add would do to a clean name: one `create` per
    /// value, carrying the record that would be written (the default TTL included).
    #[test]
    fn dry_run_add_lists_each_record_it_would_create() {
        let data = vec!["1.2.3.4".to_string(), "5.6.7.8".to_string()];
        let preview = dry_run_add_preview("example.com", "A", "www", &data, &opts(), &[]);
        assert_eq!(preview["action"], "would add");
        assert_eq!(preview["created"], 2);
        assert_eq!(preview["failed"], 0);
        let records = preview["records"].as_array().expect("records array");
        assert_eq!(records[0]["action"], "create");
        assert_eq!(records[0]["new"]["data"], "1.2.3.4");
        assert_eq!(records[0]["new"]["ttl"], 3600);
        assert_eq!(records[1]["new"]["data"], "5.6.7.8");
    }

    /// An exact duplicate of a record already in the zone is refused by the API,
    /// so the preview shows it as a failure instead of promising a create.
    #[test]
    fn dry_run_add_flags_an_exact_duplicate_as_a_failure() {
        let existing = [stored("r1", "A", "www", "1.2.3.4")];
        let data = vec!["1.2.3.4".to_string(), "5.6.7.8".to_string()];
        let preview = dry_run_add_preview("example.com", "A", "www", &data, &opts(), &existing);
        assert_eq!(preview["created"], 1);
        assert_eq!(preview["failed"], 1);
        let records = preview["records"].as_array().expect("records array");
        assert_eq!(records[0]["action"], "fail");
        assert!(
            records[0]["error"]
                .as_str()
                .is_some_and(|e| e.contains("already exists")),
            "{records:?}"
        );
        assert_eq!(records[1]["action"], "create");
    }

    /// A name that holds a CNAME can't take other types (nor the reverse), which
    /// the API refuses — the preview has to say so.
    #[test]
    fn dry_run_add_flags_a_cname_conflict_as_a_failure() {
        let existing = [stored("r1", "CNAME", "www", "target.example.com.")];
        let data = vec!["1.2.3.4".to_string()];
        let preview = dry_run_add_preview("example.com", "A", "www", &data, &opts(), &existing);
        assert_eq!(preview["failed"], 1);
        let records = preview["records"].as_array().expect("records array");
        assert_eq!(records[0]["action"], "fail");
        assert!(
            records[0]["error"]
                .as_str()
                .is_some_and(|e| e.contains("CNAME")),
            "{records:?}"
        );
    }

    /// Another value of the same type at the name is no obstacle: `add` appends.
    #[test]
    fn dry_run_add_allows_another_value_of_the_same_type() {
        let existing = [stored("r1", "A", "www", "9.9.9.9")];
        let data = vec!["1.2.3.4".to_string()];
        let preview = dry_run_add_preview("example.com", "A", "www", &data, &opts(), &existing);
        assert_eq!(preview["failed"], 0);
        assert_eq!(preview["records"][0]["action"], "create");
    }

    /// The preview survives the command's default `--fields` projection, which
    /// strips anything not listed.
    #[test]
    fn dry_run_add_survives_default_field_projection() {
        let data = vec!["1.2.3.4".to_string()];
        let preview = dry_run_add_preview("example.com", "A", "www", &data, &opts(), &[]);
        let projected = cli_engine::output::filter_fields(
            &preview,
            "domain,type,name,created,failed,action,records",
        );
        for field in [
            "domain", "type", "name", "created", "failed", "action", "records",
        ] {
            assert!(
                !projected[field].is_null(),
                "{field:?} was stripped by default_fields; preview: {projected}"
            );
        }
    }
}
