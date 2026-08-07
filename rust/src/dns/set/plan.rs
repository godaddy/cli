//! Reconcile planning for `dns set` — deciding, from the existing record ids
//! and desired `--data` values, which per-record ops to run.

/// One reconcile action for `dns set` over v3's per-record endpoints.
///
/// v3 has no atomic single-record replace: `PUT .../dns-records/{recordId}`
/// responds `200` but silently replaces the *entire* record collection with
/// just the system records and whatever's in the request body, discarding
/// everything else in the zone (reproduced live against a test zone; see
/// <https://github.com/godaddy/cli/issues/136>). So `Replace` is executed as
/// create-the-new-value-then-delete-the-old-record, never as an in-place PUT.
#[derive(Debug, PartialEq)]
pub(super) enum SetAction {
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
pub(super) fn plan_set(existing_ids: &[String], desired: &[String]) -> Vec<SetAction> {
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
}
