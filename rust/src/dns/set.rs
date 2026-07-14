//! `dns set` — replace every record for a type+name pair (destructive).

use cli_engine::{CliCoreError, CommandResult, CommandSpec, RuntimeCommandSpec, Tier};
use serde_json::{Value, json};

use crate::domain::{api_error, format_api_error, make_client, string_list};
use crate::output_schema::output_schema;
use crate::scopes::DOMAINS_DNS_UPDATE;

use domains_client::types;

use super::conflicts::{
    conflicting_records, conflicting_records_at, describe_duplicate_record, duplicate_record_issue,
};
use super::records::{
    RecordOptions, arg_bool, arg_str, fetch_records, v3_record, validate_caa_fields,
    verify_with_list_action, with_record_write_args,
};

// `dns set` reconciles over v3's per-record ops; it reports how many records it
// reused (replaced), created, and deleted to reach the desired set.
output_schema!(DnsSetResult {
    "domain": "string";
    "type": "string";
    "name": "string";
    "replaced": "number";
    "created": "number";
    "deleted": "number";
    "action": "string";
});

/// One reconcile action for `dns set` over v3's per-record endpoints.
#[derive(Debug, PartialEq)]
enum SetAction {
    /// Reuse an existing record's id, replacing its value with a new `--data`.
    Replace { record_id: String, data: String },
    /// Remove an existing record no longer wanted.
    Delete { record_id: String },
    /// Create a record for a surplus `--data` value.
    Create { data: String },
}

/// Reconcile the existing records for a type+name (their ids, in list order) with
/// the desired `--data` values: reuse ids for the overlap (`Replace`), `Delete`
/// the surplus existing, `Create` the surplus desired. Pure so the plan — the
/// least-destructive way to emulate a set-replace over per-record v3 ops — is
/// unit-testable.
fn plan_set(existing_ids: &[String], desired: &[String]) -> Vec<SetAction> {
    let overlap = existing_ids.len().min(desired.len());
    let mut actions = Vec::with_capacity(existing_ids.len().max(desired.len()));
    for i in 0..overlap {
        actions.push(SetAction::Replace {
            record_id: existing_ids[i].clone(),
            data: desired[i].clone(),
        });
    }
    for id in &existing_ids[overlap..] {
        actions.push(SetAction::Delete {
            record_id: id.clone(),
        });
    }
    for d in &desired[overlap..] {
        actions.push(SetAction::Create { data: d.clone() });
    }
    actions
}

/// Outcome of one applied `set` reconcile action, for reporting.
struct SetOutcome {
    /// What was attempted: `"replaced"`, `"created"`, or `"deleted"`.
    kind: &'static str,
    /// The `--data` value (replace/create) or record id (delete).
    detail: String,
    /// `Some(msg)` if the call failed.
    error: Option<String>,
}

impl SetOutcome {
    fn new(kind: &'static str, detail: String, error: Option<String>) -> Self {
        Self {
            kind,
            detail,
            error,
        }
    }
}

/// Build the `set` result from the applied reconcile outcomes: `Ok(json)` with
/// replaced/created/deleted counts when every action succeeded, or `Err(message)`
/// — a non-zero exit — with a per-action ✓/✗ breakdown if any failed. Pure so the
/// success/failure decision and tallies are unit-testable.
fn summarize_set_outcomes(
    domain: &str,
    record_type: &str,
    name: &str,
    outcomes: &[SetOutcome],
) -> Result<Value, String> {
    let failed = outcomes.iter().filter(|o| o.error.is_some()).count();
    let count = |k: &str| {
        outcomes
            .iter()
            .filter(|o| o.error.is_none() && o.kind == k)
            .count()
    };
    let (replaced, created, deleted) = (count("replaced"), count("created"), count("deleted"));

    if failed > 0 {
        let breakdown = outcomes
            .iter()
            .map(|o| match &o.error {
                None => format!("  ✓ {} {}", o.kind, o.detail),
                Some(e) => format!("  ✗ {} {} — {e}", o.kind, o.detail),
            })
            .collect::<Vec<_>>()
            .join("\n");
        return Err(format!(
            "set for {name} ({record_type}) partially failed — {failed} of {} action(s):\n\
             {breakdown}\n\nRe-run the original `gddy dns set` command, or `gddy dns list \
             {domain} --type {record_type} --name {name}` to review the current state.",
            outcomes.len(),
        ));
    }

    Ok(json!({
        "domain": domain,
        "type": record_type,
        "name": name,
        "replaced": replaced,
        "created": created,
        "deleted": deleted,
        "action": "set",
    }))
}

/// Delete each conflicting record — used only by `set --replace-conflicting-types`
/// to auto-resolve a name-exclusivity conflict before retrying the write.
/// Returns one `SetOutcome` per record (rather than bailing on the first
/// failure) so a partial deletion is reported explicitly, consistent with
/// `dns delete`'s per-record reporting.
async fn delete_conflicts(
    client: &domains_client::Client,
    domain: &str,
    records: &[types::DnsRecord],
    debug: bool,
) -> Vec<SetOutcome> {
    let mut outcomes = Vec::with_capacity(records.len());
    for rec in records {
        let detail = format!("{} {}", rec.type_.as_str(), rec.data);
        let Some(record_id) = rec.record_id.as_deref() else {
            outcomes.push(SetOutcome::new(
                "deleted",
                detail,
                Some(
                    "the API returned this record without a recordId, so it can't be removed \
                     automatically; remove it in the control panel"
                        .to_string(),
                ),
            ));
            continue;
        };
        let err = match client
            .delete_dns_record()
            .zone(domain)
            .record_id(record_id)
            .send()
            .await
        {
            Ok(_) => None,
            Err(e) => Some(
                api_error("deleting conflicting DNS record", debug, e)
                    .await
                    .to_string(),
            ),
        };
        outcomes.push(SetOutcome::new("deleted", detail, err));
    }
    outcomes
}

/// One `set` create/replace call, dispatched by [`send_write`].
enum WriteCall<'a> {
    Create,
    Replace { record_id: &'a str },
}

async fn send_write(
    client: &domains_client::Client,
    domain: &str,
    call: &WriteCall<'_>,
    body: types::DnsRecord,
) -> Result<(), domains_client::Error<()>> {
    match call {
        WriteCall::Create => client
            .create_dns_record()
            .zone(domain)
            .body(body)
            .send()
            .await
            .map(|_| ()),
        WriteCall::Replace { record_id } => client
            .replace_dns_record()
            .zone(domain)
            .record_id(*record_id)
            .body(body)
            .send()
            .await
            .map(|_| ()),
    }
}

/// Context for one `set` create/replace action, bundled to keep
/// [`write_with_conflict_handling`]'s signature within clippy's
/// argument-count limit.
struct WriteRequest<'a> {
    domain: &'a str,
    name: &'a str,
    record_type: &'a str,
    value: &'a str,
    opts: &'a RecordOptions,
    replace_conflicting: bool,
    debug: bool,
}

/// Apply one `set` create/replace action, with the same name-exclusivity
/// conflict handling `add` gets (see [`super::conflicts::describe_write_error`])
/// plus an opt-in auto-fix: with `--replace-conflicting-types`, a genuine
/// `DUPLICATE_RECORD` failure deletes the conflicting record(s) and retries the
/// write once. Returns the delete outcomes (if any) followed by the outcome for
/// the create/replace itself, so `outcomes.extend(...)` folds both into the same
/// reconcile summary.
async fn write_with_conflict_handling(
    client: &domains_client::Client,
    req: &WriteRequest<'_>,
    call: WriteCall<'_>,
) -> Vec<SetOutcome> {
    let (kind, action) = match &call {
        WriteCall::Create => ("created", "creating DNS record"),
        WriteCall::Replace { .. } => ("replaced", "replacing DNS record"),
    };
    let outcome = |err: Option<String>| SetOutcome::new(kind, req.value.to_string(), err);

    let body = v3_record(req.name, req.record_type, req.value, req.opts);
    let err = match send_write(client, req.domain, &call, body).await {
        Ok(()) => return vec![outcome(None)],
        Err(e) => e,
    };
    let domains_client::Error::UnexpectedResponse(resp) = err else {
        return vec![outcome(Some(
            api_error(action, req.debug, err).await.to_string(),
        ))];
    };
    let status = resp.status();
    let request_id = resp
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let resp_body = resp.text().await.unwrap_or_default();

    if !duplicate_record_issue(&resp_body) {
        let msg = format_api_error(
            action,
            status.as_u16(),
            &status.to_string(),
            &resp_body,
            request_id.as_deref(),
            req.debug,
        );
        return vec![outcome(Some(msg))];
    }

    let at_name = match conflicting_records_at(client, req.domain, req.name, req.debug).await {
        Ok(records) => records,
        Err(e) => return vec![outcome(Some(e.to_string()))],
    };
    let conflicts: Vec<types::DnsRecord> = conflicting_records(req.record_type, &at_name)
        .into_iter()
        .cloned()
        .collect();

    if !req.replace_conflicting || conflicts.is_empty() {
        // Either the caller didn't opt in, or this DUPLICATE_RECORD wasn't a
        // cross-type conflict (e.g. an exact duplicate) — nothing to auto-fix.
        let msg = describe_duplicate_record(
            req.record_type,
            req.value,
            req.domain,
            req.name,
            &at_name,
            true,
        );
        return vec![outcome(Some(msg))];
    }

    let mut outcomes = delete_conflicts(client, req.domain, &conflicts, req.debug).await;
    let retry_body = v3_record(req.name, req.record_type, req.value, req.opts);
    let retry_err = match send_write(client, req.domain, &call, retry_body).await {
        Ok(()) => None,
        Err(e) => Some(api_error(action, req.debug, e).await.to_string()),
    };
    outcomes.push(outcome(retry_err));
    outcomes
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_with_context(
        with_record_write_args(
            CommandSpec::new(
                "set",
                "Replace all records for a type+name (destructive: overwrites existing)",
            )
            .with_long(
                "Replaces every DNS record for the given type+name pair with the \
                 values supplied via `--data`, discarding any records that were \
                 there before. v3 has no bulk replace, so this reconciles over \
                 per-record calls: it reuses existing records for the overlap, \
                 deletes the surplus, and creates the shortfall (so it is not \
                 atomic — a mid-run failure is reported per record). This is \
                 destructive and irreversible — use `dns list --type <type> \
                 --name <name>` to review first. Use `dns add` to append without \
                 removing existing records.\n\
                 \n\
                 DNS only allows a CNAME or other record types at a name, never \
                 both. Setting into a name that already has the other kind fails \
                 with a specific error naming the conflict; pass \
                 `--replace-conflicting-types` to remove the conflicting record(s) \
                 automatically and retry.",
            )
            .with_system("domain")
            .with_tier(Tier::Destructive)
            .with_default_fields("domain,type,name,replaced,created,deleted")
            .with_output_schema::<DnsSetResult>()
            .with_scopes(&[DOMAINS_DNS_UPDATE]),
        )
        .with_arg(
            clap::Arg::new("replace-conflicting-types")
                .long("replace-conflicting-types")
                .action(clap::ArgAction::SetTrue)
                .help(
                    "Remove any existing CNAME (or, if setting a CNAME, any other \
                     type) at this name first",
                ),
        ),
        |ctx| async move {
            let domain = arg_str(&ctx, "domain").unwrap_or_default();
            let record_type = arg_str(&ctx, "type").unwrap_or_default();
            let name = arg_str(&ctx, "name").unwrap_or_default();
            let data = string_list(&ctx, "data");
            let opts = RecordOptions::from_ctx(&ctx);
            let replace_conflicting = arg_bool(&ctx, "replace-conflicting-types");
            validate_caa_fields(&record_type, &opts).map_err(CliCoreError::message)?;

            let debug = !ctx.middleware.debug.is_empty();
            let client = make_client(&ctx).await?;

            // Reconcile the current type+name records with the desired --data
            // set over v3's per-record ops (no atomic bulk replace exists).
            let existing = fetch_records(
                &client,
                domain.as_str(),
                Some(&record_type),
                Some(&name),
                debug,
            )
            .await?;
            // Every existing record must have a server id to reconcile against —
            // v3 mutations are keyed by recordId. Bail before touching anything
            // if any lacks one, rather than silently dropping it (which would
            // leave it behind and make `set` add records instead of replacing).
            if existing.iter().any(|r| r.record_id.is_none()) {
                return Err(CliCoreError::message(format!(
                    "some existing {record_type} records for {name} were returned without a \
                     recordId, so `set` can't safely replace the full set. Re-run `gddy dns \
                     list {domain} --type {record_type} --name {name}` and adjust in the \
                     control panel if this persists."
                )));
            }
            let existing_ids: Vec<String> = existing
                .iter()
                .filter_map(|r| r.record_id.clone())
                .collect();

            let mut outcomes = Vec::new();
            for action in plan_set(&existing_ids, &data) {
                match action {
                    SetAction::Replace {
                        record_id,
                        data: value,
                    } => {
                        let req = WriteRequest {
                            domain: domain.as_str(),
                            name: name.as_str(),
                            record_type: record_type.as_str(),
                            value: &value,
                            opts: &opts,
                            replace_conflicting,
                            debug,
                        };
                        outcomes.extend(
                            write_with_conflict_handling(
                                &client,
                                &req,
                                WriteCall::Replace {
                                    record_id: record_id.as_str(),
                                },
                            )
                            .await,
                        );
                    }
                    SetAction::Create { data: value } => {
                        let req = WriteRequest {
                            domain: domain.as_str(),
                            name: name.as_str(),
                            record_type: record_type.as_str(),
                            value: &value,
                            opts: &opts,
                            replace_conflicting,
                            debug,
                        };
                        outcomes.extend(
                            write_with_conflict_handling(&client, &req, WriteCall::Create).await,
                        );
                    }
                    SetAction::Delete { record_id } => {
                        let res = client
                            .delete_dns_record()
                            .zone(domain.as_str())
                            .record_id(record_id.as_str())
                            .send()
                            .await;
                        let err = match res {
                            Ok(_) => None,
                            Err(e) => {
                                Some(api_error("deleting DNS record", debug, e).await.to_string())
                            }
                        };
                        // Match `delete_conflicts`'s "<TYPE> <DATA>" detail format so a
                        // partial-failure breakdown doesn't mix raw record ids with
                        // human-readable values.
                        let detail = existing
                            .iter()
                            .find(|r| r.record_id.as_deref() == Some(record_id.as_str()))
                            .map(|r| format!("{record_type} {}", r.data))
                            .unwrap_or_else(|| record_id.clone());
                        outcomes.push(SetOutcome::new("deleted", detail, err));
                    }
                }
            }

            summarize_set_outcomes(&domain, &record_type, &name, &outcomes)
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
    fn plan_set_reuses_overlap_deletes_extra_creates_shortfall() {
        // 3 existing, 2 desired → reuse 2 ids, delete the 3rd.
        let existing = vec!["r1".to_string(), "r2".to_string(), "r3".to_string()];
        let desired = vec!["9.9.9.9".to_string(), "8.8.8.8".to_string()];
        assert_eq!(
            plan_set(&existing, &desired),
            vec![
                SetAction::Replace {
                    record_id: "r1".into(),
                    data: "9.9.9.9".into()
                },
                SetAction::Replace {
                    record_id: "r2".into(),
                    data: "8.8.8.8".into()
                },
                SetAction::Delete {
                    record_id: "r3".into()
                },
            ]
        );
        // 1 existing, 3 desired → reuse 1 id, create 2.
        assert_eq!(
            plan_set(
                &["r1".to_string()],
                &["a".to_string(), "b".to_string(), "c".to_string()]
            ),
            vec![
                SetAction::Replace {
                    record_id: "r1".into(),
                    data: "a".into()
                },
                SetAction::Create { data: "b".into() },
                SetAction::Create { data: "c".into() },
            ]
        );
        // none existing → all creates.
        assert_eq!(
            plan_set(&[], &["x".to_string()]),
            vec![SetAction::Create { data: "x".into() }]
        );
    }

    #[test]
    fn summarize_set_reports_counts_and_flags_partial_failure() {
        let all_ok = vec![
            SetOutcome::new("replaced", "9.9.9.9".into(), None),
            SetOutcome::new("created", "8.8.8.8".into(), None),
            SetOutcome::new("deleted", "r3".into(), None),
        ];
        let v = summarize_set_outcomes("example.com", "A", "www", &all_ok).expect("all ok");
        assert_eq!(v["replaced"], 1);
        assert_eq!(v["created"], 1);
        assert_eq!(v["deleted"], 1);

        let mixed = vec![
            SetOutcome::new("replaced", "9.9.9.9".into(), None),
            SetOutcome::new("created", "8.8.8.8".into(), Some("422 bad".into())),
        ];
        let err = summarize_set_outcomes("example.com", "A", "www", &mixed).expect_err("a failure");
        assert!(err.contains("1 of 2"), "{err}");
        assert!(err.contains("✗ created 8.8.8.8 — 422 bad"), "{err}");
    }
}
