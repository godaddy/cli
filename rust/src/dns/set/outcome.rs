//! Reporting for `dns set`: tallying applied reconcile outcomes into the
//! command's success/failure result, and building the `--dry-run` preview
//! directly from a reconcile plan.

use domains_client::types;
use serde_json::{Value, json};

use super::plan::SetAction;

/// Outcome of one applied `set` reconcile action, for reporting.
pub(super) struct SetOutcome {
    /// What was attempted: `"replaced"`, `"created"`, or `"deleted"`.
    pub(super) kind: &'static str,
    /// The `--data` value (replace/create) or record id (delete).
    pub(super) detail: String,
    /// `Some(msg)` if the call failed.
    pub(super) error: Option<String>,
}

impl SetOutcome {
    pub(super) fn new(kind: &'static str, detail: String, error: Option<String>) -> Self {
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
pub(super) fn summarize_set_outcomes(
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
pub(super) fn dry_run_set_preview(
    domain: &str,
    record_type: &str,
    name: &str,
    plan: &[SetAction],
) -> Value {
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

/// Human-readable `"<TYPE> <DATA>"` label for an existing record id, for
/// outcome reporting — falls back to the bare id if the record isn't in
/// `existing` (shouldn't happen; `existing` is what `plan_set` was built from).
pub(super) fn record_label(
    existing: &[types::DnsRecord],
    record_type: &str,
    record_id: &str,
) -> String {
    existing
        .iter()
        .find(|r| r.record_id.as_deref() == Some(record_id))
        .map(|r| format!("{record_type} {}", r.data.as_deref().unwrap_or("(no data)")))
        .unwrap_or_else(|| record_id.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dns::set::plan::plan_set;

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

    /// Proves `view_columns()`'s field names actually match what
    /// `dry_run_set_preview` emits — a mismatch here would silently drop the
    /// reconcile plan from human output.
    #[test]
    fn dry_run_set_preview_renders_plan_as_a_nested_table() {
        let plan = plan_set(
            &["r1".to_string()],
            &["9.9.9.9".to_string(), "8.8.8.8".to_string()],
        );
        let preview = dry_run_set_preview("example.com", "A", "www", &plan);
        let envelope = cli_engine::Envelope::success(preview, "domain");
        let rendered =
            cli_engine::render_human_with_view(&envelope, Some(&super::super::view_columns()), "");
        assert!(rendered.contains("Plan:"), "{rendered}");
        assert!(rendered.contains("RECORD ID"), "{rendered}");
        assert!(rendered.contains("replace"), "{rendered}");
        assert!(rendered.contains("create"), "{rendered}");
        assert!(rendered.contains("9.9.9.9"), "{rendered}");
    }
}
