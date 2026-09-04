//! `dns add` — append new DNS records to a domain without touching existing ones.

use cli_engine::{Alignment, CommandResult, CommandSpec, RuntimeCommandSpec, TableColumn, Tier};
use serde_json::{Value, json};

use crate::domain::make_client;
use crate::output_schema::output_schema;
use crate::scopes::DOMAINS_DNS_UPDATE;

use super::conflicts::{WriteErrorContext, describe_write_error};
use super::records::{
    RecordOptions, RecordWriteArgs, v3_records, validate_caa_fields, validate_svcb_fields,
    validate_tlsa_fields, verify_with_list_action,
};

// `dns add` creates one v3 record per `--data` value; it reports each outcome
// individually so a partial failure is explicit (see the handler).
output_schema!(DnsAddResult {
    "domain": "string";
    "type": "string";
    "name": "string";
    "created": "number";
    "failed": "number";
    "results": "[]object";
    "action": "string";
});

/// Summarize the per-record create outcomes of `dns add` (each `--data` value
/// paired with `Ok(())` or an `Err(message)`), preserving input order. Returns
/// the success JSON payload when *every* record was created, or an error message
/// (a non-zero exit) with a per-record breakdown if any failed. Pure — no I/O —
/// so the aggregation and the success/failure decision are unit-testable.
fn summarize_add_outcomes(
    domain: &str,
    record_type: &str,
    name: &str,
    outcomes: Vec<(String, Result<(), String>)>,
) -> Result<Value, String> {
    let mut results = Vec::with_capacity(outcomes.len());
    let mut failed = 0usize;
    for (value, outcome) in &outcomes {
        match outcome {
            Ok(()) => results.push(json!({ "data": value, "status": "created" })),
            Err(err) => {
                failed += 1;
                results.push(json!({ "data": value, "status": "failed", "error": err }));
            }
        }
    }
    let total = results.len();
    let created = total - failed;

    if failed > 0 {
        let breakdown = outcomes
            .iter()
            .map(|(value, outcome)| match outcome {
                Ok(()) => format!("  ✓ {value} — created"),
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

    Ok(json!({
        "domain": domain,
        "type": record_type,
        "name": name,
        "created": created,
        "failed": failed,
        "results": results,
        "action": "add",
    }))
}

/// `results` isn't in the default fields, but renders as an indented child
/// table instead of a raw JSON dump when selected via `--fields results` (or
/// `--fields all`).
fn view_columns() -> Vec<TableColumn> {
    vec![
        TableColumn::new("domain", "Domain"),
        TableColumn::new("type", "Type"),
        TableColumn::new("name", "Name"),
        TableColumn::new("created", "Created").align(Alignment::Right),
        TableColumn::new("failed", "Failed").align(Alignment::Right),
        TableColumn::new("results", "Results").nested(vec![
            TableColumn::new("data", "Data"),
            TableColumn::new("status", "Status"),
            TableColumn::new("error", "Error"),
        ]),
        TableColumn::new("action", "Action"),
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
                     multiple records for the same type+name (each is a separate v3 \
                     create call). `--ttl` defaults to 3600 when omitted. DNS only \
                     allows a CNAME or other record types at a name, never both — \
                     adding into a name that already has the other kind fails with a \
                     specific error naming the conflicting record. Use `dns set` to \
                     replace the full record set for a type+name.",
        )
        .with_system("domain")
        .with_tier(Tier::Mutate)
        .with_default_fields("domain,type,name,created,failed")
        .with_output_schema::<DnsAddResult>()
        .with_view(view_columns())
        .with_scopes(&[DOMAINS_DNS_UPDATE]),
        |ctx, args: RecordWriteArgs| async move {
            let opts = RecordOptions::from_write_args(&args);
            let domain = args.domain;
            let record_type = args.record_type;
            let name = args.name;
            let data = args.data;
            validate_caa_fields(&record_type, &opts)
                .map_err(crate::error::GddyError::validation)?;
            validate_tlsa_fields(&record_type, &opts)
                .map_err(crate::error::GddyError::validation)?;
            validate_svcb_fields(&record_type, &opts)
                .map_err(crate::error::GddyError::validation)?;
            let records = v3_records(&name, &record_type, &data, &opts);

            let debug = !ctx.middleware.debug.is_empty();
            let client = make_client(&ctx).await?;
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
                    Ok(_) => Ok(()),
                    Err(e) => Err(describe_write_error(
                        &client,
                        &WriteErrorContext {
                            domain: domain.as_str(),
                            name: name.as_str(),
                            desired_type: record_type.as_str(),
                            desired_data: value.as_str(),
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
                .map(|v| {
                    CommandResult::new(v).with_next_actions(vec![verify_with_list_action(
                        &domain,
                        &record_type,
                        &name,
                    )])
                })
                .map_err(super::mutate_failed)
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dns_add_all_created_reports_each_record() {
        let payload = summarize_add_outcomes(
            "example.com",
            "A",
            "www",
            vec![
                ("1.2.3.4".to_string(), Ok(())),
                ("5.6.7.8".to_string(), Ok(())),
            ],
        )
        .expect("all created -> success payload");
        assert_eq!(payload["created"], 2);
        assert_eq!(payload["failed"], 0);
        let results = payload["results"].as_array().expect("results array");
        assert_eq!(results.len(), 2);
        assert_eq!(results[0]["data"], "1.2.3.4");
        assert_eq!(results[0]["status"], "created");
        assert_eq!(results[1]["data"], "5.6.7.8");
    }

    /// Proves `view_columns()`'s field names actually match what
    /// `summarize_add_outcomes` emits — a mismatch here would silently drop
    /// the per-record breakdown from human output.
    #[test]
    fn dns_add_results_render_as_a_nested_table() {
        let payload = summarize_add_outcomes(
            "example.com",
            "A",
            "www",
            vec![("1.2.3.4".to_string(), Ok(()))],
        )
        .expect("all created -> success payload");
        let envelope = cli_engine::Envelope::success(payload, "domain");
        let rendered = cli_engine::render_human_with_view(&envelope, Some(&view_columns()), "");
        assert!(rendered.contains("Results:"), "{rendered}");
        assert!(rendered.contains("1.2.3.4"), "{rendered}");
        assert!(rendered.contains("created"), "{rendered}");
    }

    #[test]
    fn dns_add_partial_failure_is_an_error_with_per_record_breakdown() {
        // Middle record fails: the command must NOT succeed, and the message must
        // make clear which values were created and which failed (and why).
        let err = summarize_add_outcomes(
            "example.com",
            "A",
            "www",
            vec![
                ("1.2.3.4".to_string(), Ok(())),
                ("5.6.7.8".to_string(), Err("422 invalid data".to_string())),
                ("9.9.9.9".to_string(), Ok(())),
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
}
