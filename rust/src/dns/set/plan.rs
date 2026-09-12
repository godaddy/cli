//! Reconcile planning for `dns set` — deciding, from the existing records
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

/// Reconcile the existing records for a type+name — `(record_id, current value)`
/// in list order — with the desired `--data` values: pair them up (`Replace`),
/// `Delete` the surplus existing, `Create` the surplus desired. Pure so the
/// plan — the least-destructive way to emulate a set-replace over per-record v3
/// ops — is unit-testable.
///
/// A desired value that some existing record already holds is paired with *that*
/// record before anything is paired by position. v3 keys record identity on
/// (name, type, data), so creating a value another record still holds fails with
/// `DUPLICATE_RECORD`. Under a purely positional pairing that is not a harmless
/// no-op: [`super::write::apply_replace`] keeps the old record when its create
/// fails, while the surplus `Delete` later in the plan still runs — so narrowing
/// `www A {1.2.3.4, 5.6.7.8}` to just `5.6.7.8` would delete `5.6.7.8` and leave
/// `1.2.3.4` behind, the exact inverse of what was asked. Pairing retained values
/// first means a value that stays in the set is never re-created.
pub(super) fn plan_set(existing: &[(String, String)], desired: &[String]) -> Vec<SetAction> {
    let mut paired: Vec<Option<usize>> = vec![None; desired.len()];
    let mut taken = vec![false; existing.len()];

    // Pass 1: pair by content, wherever the holder sits in the list.
    for (d, want) in desired.iter().enumerate() {
        if let Some(e) = (0..existing.len()).find(|&e| !taken[e] && &existing[e].1 == want) {
            taken[e] = true;
            paired[d] = Some(e);
        }
    }

    // Pass 2: whatever is left pairs by position, as before.
    let mut free: Vec<usize> = (0..existing.len()).filter(|&e| !taken[e]).rev().collect();
    for slot in paired.iter_mut().filter(|s| s.is_none()) {
        let Some(e) = free.pop() else { break };
        taken[e] = true;
        *slot = Some(e);
    }

    let mut actions = Vec::with_capacity(existing.len().max(desired.len()));
    for (d, want) in desired.iter().enumerate() {
        if let Some(e) = paired[d] {
            actions.push(SetAction::Replace {
                record_id: existing[e].0.clone(),
                data: want.clone(),
            });
        }
    }
    // Deletes stay ahead of creates: freeing a surplus record's value is what
    // lets a create of that same value succeed.
    for (e, (id, _)) in existing.iter().enumerate() {
        if !taken[e] {
            actions.push(SetAction::Delete {
                record_id: id.clone(),
            });
        }
    }
    for (d, want) in desired.iter().enumerate() {
        if paired[d].is_none() {
            actions.push(SetAction::Create { data: want.clone() });
        }
    }
    actions
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `(record_id, current value)` pairs, as `plan_set` takes them.
    fn ids(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(id, value)| ((*id).to_string(), (*value).to_string()))
            .collect()
    }

    #[test]
    fn plan_set_reuses_overlap_deletes_extra_creates_shortfall() {
        // 3 existing, 2 desired, no value in common → reuse 2 ids, delete the 3rd.
        let existing = ids(&[("r1", "1.1.1.1"), ("r2", "2.2.2.2"), ("r3", "3.3.3.3")]);
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
                &ids(&[("r1", "z")]),
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
    fn plan_set_pairs_a_retained_value_with_the_record_that_holds_it() {
        // Narrowing `www A {1.2.3.4, 5.6.7.8}` down to just 5.6.7.8 must pair the
        // desired value with r2, which already holds it, and delete r1. Pairing
        // positionally instead (Replace{r1 → 5.6.7.8}, Delete{r2}) makes the
        // create collide with r2's still-live duplicate, so `apply_replace` keeps
        // r1 while the surplus delete still removes r2 — leaving the zone holding
        // exactly the value the user asked to drop.
        let existing = ids(&[("r1", "1.2.3.4"), ("r2", "5.6.7.8")]);
        assert_eq!(
            plan_set(&existing, &["5.6.7.8".to_string()]),
            vec![
                SetAction::Replace {
                    record_id: "r2".into(),
                    data: "5.6.7.8".into()
                },
                SetAction::Delete {
                    record_id: "r1".into()
                },
            ]
        );
    }

    #[test]
    fn plan_set_reordering_the_same_values_pairs_each_to_itself() {
        // A pure reorder is a no-op set. Every pairing must land on the record
        // that already holds the value, so `apply_replace` short-circuits each
        // one instead of issuing two creates that both fail as duplicates.
        let existing = ids(&[("r1", "1.2.3.4"), ("r2", "5.6.7.8")]);
        assert_eq!(
            plan_set(&existing, &["5.6.7.8".to_string(), "1.2.3.4".to_string()]),
            vec![
                SetAction::Replace {
                    record_id: "r2".into(),
                    data: "5.6.7.8".into()
                },
                SetAction::Replace {
                    record_id: "r1".into(),
                    data: "1.2.3.4".into()
                },
            ]
        );
    }

    #[test]
    fn plan_set_keeps_a_retained_value_and_still_replaces_the_rest() {
        // Mixed case: 5.6.7.8 stays (pair with r2), 1.2.3.4 goes, 9.9.9.9 is new.
        // The freed id r1 is reused positionally rather than deleted-and-created.
        let existing = ids(&[("r1", "1.2.3.4"), ("r2", "5.6.7.8")]);
        assert_eq!(
            plan_set(&existing, &["5.6.7.8".to_string(), "9.9.9.9".to_string()]),
            vec![
                SetAction::Replace {
                    record_id: "r2".into(),
                    data: "5.6.7.8".into()
                },
                SetAction::Replace {
                    record_id: "r1".into(),
                    data: "9.9.9.9".into()
                },
            ]
        );
    }

    #[test]
    fn plan_set_duplicate_desired_values_pair_distinct_records() {
        // Two identical --data values must not both pair with the same record.
        let existing = ids(&[("r1", "1.2.3.4"), ("r2", "1.2.3.4")]);
        assert_eq!(
            plan_set(&existing, &["1.2.3.4".to_string(), "1.2.3.4".to_string()]),
            vec![
                SetAction::Replace {
                    record_id: "r1".into(),
                    data: "1.2.3.4".into()
                },
                SetAction::Replace {
                    record_id: "r2".into(),
                    data: "1.2.3.4".into()
                },
            ]
        );
    }
}
