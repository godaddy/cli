//! `dns set` — replace every record for a type+name pair (destructive).

use cli_engine::{CommandResult, CommandSpec, RuntimeCommandSpec, Tier};
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
// replaced, created, and deleted to reach the desired set.
output_schema!(DnsSetResult {
    "domain": "string";
    "type": "string";
    "name": "string";
    "replaced": "number";
    "created": "number";
    "deleted": "number";
    "action": "string";
    "plan": "[]object", optional;
});

/// One reconcile action for `dns set` over v3's per-record endpoints.
///
/// v3 has no atomic single-record replace: `PUT .../dns-records/{recordId}`
/// responds `200` but silently replaces the *entire* record collection with
/// just the system records and whatever's in the request body, discarding
/// everything else in the zone (reproduced live against a test zone; see
/// <https://github.com/godaddy/cli/issues/136>). So `Replace` is executed as
/// create-the-new-value-then-delete-the-old-record, never as an in-place PUT.
#[derive(Debug, PartialEq)]
enum SetAction {
    /// Create a record for the desired `--data` value, then delete the
    /// existing record (`record_id`) it's replacing.
    Replace { record_id: String, data: String },
    /// Remove an existing record no longer wanted.
    Delete { record_id: String },
    /// Create a record for a surplus `--data` value.
    Create { data: String },
}

/// Reconcile the existing records for a type+name (their ids, in list order) with
/// the desired `--data` values: pair up the overlap (`Replace`), `Delete`
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

/// Builds the `dns set --dry-run` preview directly from the same reconcile
/// plan the real execution would apply, so the preview can never drift from
/// what `set` would actually do. Doesn't attempt to predict a name-exclusivity
/// conflict (that's only known once a write is actually attempted) — the
/// preview shows the reconcile plan, not whether `--replace-conflicting-types`
/// would end up mattering.
fn dry_run_set_preview(domain: &str, record_type: &str, name: &str, plan: &[SetAction]) -> Value {
    let count = |predicate: fn(&SetAction) -> bool| plan.iter().filter(|a| predicate(a)).count();
    let plan_json: Vec<Value> = plan
        .iter()
        .map(|action| match action {
            SetAction::Replace { record_id, data } => {
                json!({"action": "replace", "recordId": record_id, "data": data})
            }
            SetAction::Create { data } => json!({"action": "create", "data": data}),
            SetAction::Delete { record_id } => json!({"action": "delete", "recordId": record_id}),
        })
        .collect();

    json!({
        "domain": domain,
        "type": record_type,
        "name": name,
        // Reuse DnsSetResult's own field names (rather than inventing
        // "wouldReplace" etc.) so the preview survives the command's
        // `with_default_fields` projection instead of being silently
        // stripped down to just domain/type/name.
        "replaced": count(|a| matches!(a, SetAction::Replace { .. })),
        "created": count(|a| matches!(a, SetAction::Create { .. })),
        "deleted": count(|a| matches!(a, SetAction::Delete { .. })),
        "plan": plan_json,
        "action": "would set",
    })
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

/// Context for one `set` create action, bundled to keep
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

/// Create one record for `req.value` (a surplus `--data` value, or — via
/// [`apply_replace`] — the new half of a replace pairing), with the same
/// name-exclusivity conflict handling `add` gets (see
/// [`super::conflicts::describe_write_error`]) plus an opt-in auto-fix: with
/// `--replace-conflicting-types`, a genuine `DUPLICATE_RECORD` failure deletes
/// the conflicting record(s) and retries the write once. Returns the delete
/// outcomes (if any) followed by the outcome for the create itself, so
/// `outcomes.extend(...)` folds both into the same reconcile summary. The
/// terminal outcome's `kind` is always `"created"` — [`apply_replace`]
/// relabels it to `"replaced"` when this is the create half of a replace.
async fn write_with_conflict_handling(
    client: &domains_client::Client,
    req: &WriteRequest<'_>,
) -> Vec<SetOutcome> {
    const ACTION: &str = "creating DNS record";
    let outcome = |err: Option<String>| SetOutcome::new("created", req.value.to_string(), err);

    let body = v3_record(req.name, req.record_type, req.value, req.opts);
    let err = match client
        .create_dns_record()
        .zone(req.domain)
        .body(body)
        .send()
        .await
    {
        Ok(_) => return vec![outcome(None)],
        Err(e) => e,
    };
    let domains_client::Error::UnexpectedResponse(resp) = err else {
        return vec![outcome(Some(
            api_error(ACTION, req.debug, err).await.to_string(),
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
            ACTION,
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
    let retry_err = match client
        .create_dns_record()
        .zone(req.domain)
        .body(retry_body)
        .send()
        .await
    {
        Ok(_) => None,
        Err(e) => Some(api_error(ACTION, req.debug, e).await.to_string()),
    };
    outcomes.push(outcome(retry_err));
    outcomes
}

/// Human-readable `"<TYPE> <DATA>"` label for an existing record id, for
/// outcome reporting — falls back to the bare id if the record isn't in
/// `existing` (shouldn't happen; `existing` is what `plan_set` was built from).
fn record_label(existing: &[types::DnsRecord], record_type: &str, record_id: &str) -> String {
    existing
        .iter()
        .find(|r| r.record_id.as_deref() == Some(record_id))
        .map(|r| format!("{record_type} {}", r.data))
        .unwrap_or_else(|| record_id.to_string())
}

/// Apply one `set` "replace" reconcile action: create the new value via
/// [`write_with_conflict_handling`], then — only if that succeeded — delete
/// the record it's replacing. There's no atomic single-record replace in v3
/// (see the note on [`SetAction::Replace`]), so this create-then-delete pair
/// is the least-destructive way to emulate one: if the create fails, the old
/// record is left untouched rather than risking data loss.
///
/// `old_data` is the record's *current* value, if known — when it already
/// equals `req.value` (a no-op `set`, e.g. re-running the same command, or
/// only some of several values actually changed) this is a no-op: creating
/// the "new" value while the identical old record still exists would fail
/// with v3's exact-duplicate `DUPLICATE_RECORD` (see
/// [`super::conflicts::describe_duplicate_record`]), which isn't a real
/// failure, just this pairing having nothing to do.
///
/// Relabels the create outcome's `kind` from `"created"` to `"replaced"` so
/// `summarize_set_outcomes`'s tallies mean what the user asked for. If the
/// delete of the old record fails after a successful create, that's reported
/// as an extra `"cleanup"`-kind outcome — a kind `summarize_set_outcomes`'s
/// counts don't recognize, so it doesn't inflate replaced/created/deleted,
/// but its `error` still fails the command and shows up in the breakdown.
async fn apply_replace(
    client: &domains_client::Client,
    req: &WriteRequest<'_>,
    old_record_id: &str,
    old_detail: &str,
    old_data: Option<&str>,
) -> Vec<SetOutcome> {
    if old_data == Some(req.value) {
        return vec![SetOutcome::new("replaced", req.value.to_string(), None)];
    }

    let mut outcomes = write_with_conflict_handling(client, req).await;
    let Some(last) = outcomes.last_mut() else {
        return outcomes;
    };
    last.kind = "replaced";
    if last.error.is_some() {
        return outcomes;
    }

    if let Err(e) = client
        .delete_dns_record()
        .zone(req.domain)
        .record_id(old_record_id)
        .send()
        .await
    {
        let msg = format!(
            "the new value was created, but the previous record ({old_detail}) could not be \
             removed: {}; run `gddy dns list {} --type {} --name {}` to review the current \
             state and remove it manually if it's still there",
            api_error("deleting the previous DNS record", req.debug, e).await,
            req.domain,
            req.record_type,
            req.name,
        );
        outcomes.push(SetOutcome::new(
            "cleanup",
            old_detail.to_string(),
            Some(msg),
        ));
    }
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
                 there before. v3 has no bulk or in-place replace, so this \
                 reconciles over per-record calls: for the overlap it creates \
                 the replacement value then deletes the record it's replacing, \
                 it deletes the surplus, and it creates the shortfall (so it is \
                 not atomic — a mid-run failure is reported per record). This is \
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
            .handles_dry_run(true)
            .with_default_fields("domain,type,name,replaced,created,deleted,action,plan")
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
            validate_caa_fields(&record_type, &opts)
                .map_err(crate::error::GddyError::validation)?;

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
                let message = format!(
                    "some existing {record_type} records for {name} were returned without a \
                     recordId, so `set` can't safely replace the full set. Re-run `gddy dns \
                     list {domain} --type {record_type} --name {name}` and adjust in the \
                     control panel if this persists."
                );
                let fix = format!(
                    "Re-run `gddy dns list {domain} --type {record_type} --name {name}` and \
                     adjust in the control panel if this persists."
                );
                return Err(crate::error::GddyError::unexpected(message)
                    .with_fix(fix)
                    .into_cli_error());
            }
            let existing_ids: Vec<String> = existing
                .iter()
                .filter_map(|r| r.record_id.clone())
                .collect();

            let plan = plan_set(&existing_ids, &data);
            if ctx.dry_run() {
                return Ok(CommandResult::new(dry_run_set_preview(
                    &domain,
                    &record_type,
                    &name,
                    &plan,
                ))
                .with_dry_run());
            }

            let mut outcomes = Vec::new();
            for action in plan {
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
                        let old_record = existing
                            .iter()
                            .find(|r| r.record_id.as_deref() == Some(record_id.as_str()));
                        let old_detail = record_label(&existing, &record_type, &record_id);
                        outcomes.extend(
                            apply_replace(
                                &client,
                                &req,
                                &record_id,
                                &old_detail,
                                old_record.map(|r| r.data.as_str()),
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
                        outcomes.extend(write_with_conflict_handling(&client, &req).await);
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
                        let detail = record_label(&existing, &record_type, &record_id);
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
                .map_err(super::mutate_failed)
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

        // A `apply_replace`-style "cleanup" outcome (the new value was created
        // but the old record couldn't be deleted afterward) still fails the
        // command, but must not be double-counted into `replaced`/`deleted`
        // since the replace itself already succeeded.
        let replace_with_cleanup_failure = vec![
            SetOutcome::new("replaced", "9.9.9.9".into(), None),
            SetOutcome::new("cleanup", "A 1.2.3.4".into(), Some("still there".into())),
        ];
        let err = summarize_set_outcomes("example.com", "A", "www", &replace_with_cleanup_failure)
            .expect_err("cleanup failure still fails the command");
        assert!(err.contains("1 of 2"), "{err}");
        assert!(err.contains("✗ cleanup A 1.2.3.4 — still there"), "{err}");
    }

    #[test]
    fn dry_run_set_preview_matches_the_plan_it_would_execute() {
        let plan = plan_set(
            &["r1".to_string(), "r2".to_string(), "r3".to_string()],
            &["9.9.9.9".to_string(), "8.8.8.8".to_string()],
        );
        let preview = dry_run_set_preview("example.com", "A", "www", &plan);
        assert_eq!(preview["replaced"], 2);
        assert_eq!(preview["created"], 0);
        assert_eq!(preview["deleted"], 1);
        assert_eq!(preview["action"], "would set");
        assert_eq!(preview["plan"].as_array().expect("plan array").len(), 3);
    }

    /// The command's `--output json` path always projects through
    /// `default_fields` when the user doesn't pass `--fields` — a preview
    /// field that isn't in that list is silently stripped and never reaches
    /// the user. Prove the preview's summary fields actually survive it
    /// (this is exactly the gap a pure call to `dry_run_set_preview` can't
    /// catch, since it never goes through the projection).
    #[test]
    fn dry_run_set_preview_survives_default_field_projection() {
        let plan = plan_set(&["r1".to_string()], &["9.9.9.9".to_string()]);
        let preview = dry_run_set_preview("example.com", "A", "www", &plan);
        let default_fields = "domain,type,name,replaced,created,deleted,action,plan";
        let projected = cli_engine::output::filter_fields(&preview, default_fields);
        for field in [
            "domain", "type", "name", "replaced", "created", "deleted", "action", "plan",
        ] {
            assert!(
                !projected[field].is_null(),
                "{field:?} was stripped by default_fields; preview: {projected}"
            );
        }
    }

    // --- write_with_conflict_handling ---------------------------------------
    //
    // These exercise the DUPLICATE_RECORD branching against a mock server: the
    // no-conflict path, the conflict-without-`--replace-conflicting-types` path
    // (message only, no delete attempted), and the conflict-with-the-flag path
    // (delete_conflicts outcome then the retry outcome, in that order).

    use httpmock::prelude::*;

    fn client_for(server: &MockServer) -> domains_client::Client {
        domains_client::client_with_auth(
            &server.base_url(),
            "Bearer tok",
            "godaddy-cli/test",
            "req-1",
        )
        .expect("build client")
    }

    fn write_opts() -> RecordOptions {
        RecordOptions {
            ttl: None,
            priority: None,
            port: None,
            weight: None,
            protocol: None,
            service: None,
            flag: None,
            tag: None,
        }
    }

    const DUPLICATE_RECORD_BODY: &str = r#"{"correlationId":"c-1","details":[{"issue":"DUPLICATE_RECORD","description":"Duplicate data provided for record name, www."}],"message":"Request failed validation","name":"VALIDATION_ERROR"}"#;

    #[tokio::test]
    async fn write_with_conflict_handling_creates_without_conflict() {
        let server = MockServer::start_async().await;
        let create = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v3/domains/zones/example.com/dns-records");
                then.status(201).json_body(
                    json!({ "type": "A", "name": "www", "data": "1.2.3.4", "ttl": 3600 }),
                );
            })
            .await;

        let opts = write_opts();
        let req = WriteRequest {
            domain: "example.com",
            name: "www",
            record_type: "A",
            value: "1.2.3.4",
            opts: &opts,
            replace_conflicting: false,
            debug: false,
        };
        let outcomes = write_with_conflict_handling(&client_for(&server), &req).await;

        create.assert_async().await;
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].kind, "created");
        assert!(outcomes[0].error.is_none(), "{:?}", outcomes[0].error);
    }

    #[tokio::test]
    async fn write_with_conflict_handling_conflict_without_flag_reports_message_and_skips_delete() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v3/domains/zones/example.com/dns-records");
                then.status(422).json_body(
                    serde_json::from_str::<Value>(DUPLICATE_RECORD_BODY).expect("valid json"),
                );
            })
            .await;
        server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/v3/domains/zones/example.com/dns-records")
                    .query_param("name", "www");
                then.status(200).json_body(json!({
                    "items": [
                        { "type": "CNAME", "name": "www", "data": "parkpage.godaddy.com", "ttl": 3600, "recordId": "cn-1" }
                    ],
                    "totalItems": 1,
                    "totalPages": 1,
                    "links": []
                }));
            })
            .await;

        let opts = write_opts();
        let req = WriteRequest {
            domain: "example.com",
            name: "www",
            record_type: "A",
            value: "1.2.3.4",
            opts: &opts,
            replace_conflicting: false,
            debug: false,
        };
        let outcomes = write_with_conflict_handling(&client_for(&server), &req).await;

        // Only the message outcome — no delete attempted since the flag wasn't set.
        assert_eq!(outcomes.len(), 1);
        let err = outcomes[0].error.as_deref().expect("conflict reported");
        assert!(err.contains("already has a CNAME record"), "{err}");
        assert!(err.contains("--replace-conflicting-types"), "{err}");
    }

    #[tokio::test]
    async fn write_with_conflict_handling_replace_flag_deletes_then_retries_in_order() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v3/domains/zones/example.com/dns-records");
                then.status(422).json_body(
                    serde_json::from_str::<Value>(DUPLICATE_RECORD_BODY).expect("valid json"),
                );
            })
            .await;
        server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/v3/domains/zones/example.com/dns-records")
                    .query_param("name", "www");
                then.status(200).json_body(json!({
                    "items": [
                        { "type": "CNAME", "name": "www", "data": "parkpage.godaddy.com", "ttl": 3600, "recordId": "cn-1" }
                    ],
                    "totalItems": 1,
                    "totalPages": 1,
                    "links": []
                }));
            })
            .await;
        let delete = server
            .mock_async(|when, then| {
                when.method(DELETE)
                    .path("/v3/domains/zones/example.com/dns-records/cn-1");
                then.status(204);
            })
            .await;

        let opts = write_opts();
        let req = WriteRequest {
            domain: "example.com",
            name: "www",
            record_type: "A",
            value: "1.2.3.4",
            opts: &opts,
            replace_conflicting: true,
            debug: false,
        };
        let outcomes = write_with_conflict_handling(&client_for(&server), &req).await;

        delete.assert_async().await;
        // delete_conflicts' outcome must come first, then the retry outcome —
        // the order `summarize_set_outcomes`'s breakdown reports to the user.
        assert_eq!(outcomes.len(), 2);
        assert_eq!(outcomes[0].kind, "deleted");
        assert_eq!(outcomes[0].detail, "CNAME parkpage.godaddy.com");
        assert!(outcomes[0].error.is_none(), "{:?}", outcomes[0].error);
        assert_eq!(outcomes[1].kind, "created");
        assert_eq!(outcomes[1].detail, "1.2.3.4");
        // The retry hits the same mocked conflict response; unlike the first
        // attempt, a failure here is reported generically (no re-diagnosis).
        let err = outcomes[1].error.as_deref().expect("retry still failed");
        assert!(err.contains("Duplicate data provided"), "{err}");
    }

    // --- apply_replace -------------------------------------------------------
    //
    // `set`'s "replace" semantic is create-the-new-value-then-delete-the-old
    // (v3 has no atomic single-record replace; see GH #136). These exercise:
    // the happy path (create then delete, one "replaced" outcome), the create
    // failing (delete must never be attempted — the old record stays intact),
    // and the delete failing after a successful create (reported as a
    // separate "cleanup" outcome that still fails the command).

    fn replace_req<'a>(opts: &'a RecordOptions, value: &'a str) -> WriteRequest<'a> {
        WriteRequest {
            domain: "example.com",
            name: "www",
            record_type: "A",
            value,
            opts,
            replace_conflicting: false,
            debug: false,
        }
    }

    #[tokio::test]
    async fn apply_replace_creates_then_deletes_the_old_record() {
        let server = MockServer::start_async().await;
        let create = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v3/domains/zones/example.com/dns-records");
                then.status(201).json_body(
                    json!({ "type": "A", "name": "www", "data": "9.9.9.9", "ttl": 3600 }),
                );
            })
            .await;
        let delete = server
            .mock_async(|when, then| {
                when.method(DELETE)
                    .path("/v3/domains/zones/example.com/dns-records/old-1");
                then.status(204);
            })
            .await;

        let opts = write_opts();
        let req = replace_req(&opts, "9.9.9.9");
        let outcomes = apply_replace(
            &client_for(&server),
            &req,
            "old-1",
            "A 1.2.3.4",
            Some("1.2.3.4"),
        )
        .await;

        create.assert_async().await;
        delete.assert_async().await;
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].kind, "replaced");
        assert!(outcomes[0].error.is_none(), "{:?}", outcomes[0].error);
    }

    /// The record being "replaced" already holds the desired value (e.g. a
    /// re-run of the same `set`, or only some of several `--data` values
    /// actually changed) — creating it again would hit v3's exact-duplicate
    /// `DUPLICATE_RECORD` since the old record hasn't been deleted yet, so
    /// this must short-circuit as a no-op success without any API calls.
    #[tokio::test]
    async fn apply_replace_is_a_no_op_when_old_record_already_has_the_desired_value() {
        let server = MockServer::start_async().await;
        let create = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v3/domains/zones/example.com/dns-records");
                then.status(201);
            })
            .await;
        let delete = server
            .mock_async(|when, then| {
                when.method(DELETE)
                    .path("/v3/domains/zones/example.com/dns-records/old-1");
                then.status(204);
            })
            .await;

        let opts = write_opts();
        let req = replace_req(&opts, "1.2.3.4");
        let outcomes = apply_replace(
            &client_for(&server),
            &req,
            "old-1",
            "A 1.2.3.4",
            Some("1.2.3.4"),
        )
        .await;

        assert_eq!(
            create.hits_async().await,
            0,
            "no create for a no-op replace"
        );
        assert_eq!(
            delete.hits_async().await,
            0,
            "no delete for a no-op replace"
        );
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].kind, "replaced");
        assert_eq!(outcomes[0].detail, "1.2.3.4");
        assert!(outcomes[0].error.is_none(), "{:?}", outcomes[0].error);
    }

    #[tokio::test]
    async fn apply_replace_leaves_old_record_alone_when_create_fails() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v3/domains/zones/example.com/dns-records");
                then.status(500);
            })
            .await;
        let delete = server
            .mock_async(|when, then| {
                when.method(DELETE)
                    .path("/v3/domains/zones/example.com/dns-records/old-1");
                then.status(204);
            })
            .await;

        let opts = write_opts();
        let req = replace_req(&opts, "9.9.9.9");
        let outcomes = apply_replace(
            &client_for(&server),
            &req,
            "old-1",
            "A 1.2.3.4",
            Some("1.2.3.4"),
        )
        .await;

        assert_eq!(
            delete.hits_async().await,
            0,
            "the old record must not be touched when the create fails"
        );
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].kind, "replaced");
        assert!(outcomes[0].error.is_some());
    }

    #[tokio::test]
    async fn apply_replace_reports_cleanup_outcome_when_delete_of_old_record_fails() {
        let server = MockServer::start_async().await;
        let create = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v3/domains/zones/example.com/dns-records");
                then.status(201).json_body(
                    json!({ "type": "A", "name": "www", "data": "9.9.9.9", "ttl": 3600 }),
                );
            })
            .await;
        let delete = server
            .mock_async(|when, then| {
                when.method(DELETE)
                    .path("/v3/domains/zones/example.com/dns-records/old-1");
                then.status(404).json_body(json!({
                    "correlationId": "c-1",
                    "message": "Record not found in DNS zone.",
                    "name": "RECORD_NOT_FOUND"
                }));
            })
            .await;

        let opts = write_opts();
        let req = replace_req(&opts, "9.9.9.9");
        let outcomes = apply_replace(
            &client_for(&server),
            &req,
            "old-1",
            "A 1.2.3.4",
            Some("1.2.3.4"),
        )
        .await;

        create.assert_async().await;
        delete.assert_async().await;
        assert_eq!(outcomes.len(), 2);
        assert_eq!(outcomes[0].kind, "replaced");
        assert!(outcomes[0].error.is_none(), "{:?}", outcomes[0].error);
        assert_eq!(outcomes[1].kind, "cleanup");
        let err = outcomes[1]
            .error
            .as_deref()
            .expect("cleanup failure reported");
        assert!(err.contains("A 1.2.3.4"), "{err}");
        assert!(err.contains("dns list"), "{err}");
    }
}
