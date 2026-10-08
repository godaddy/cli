//! Reconcile planning for `dns set` — deciding, from the existing records
//! and desired `--data` values, which per-record ops to run.

use domains_client::types;

use crate::dns::records::{record_value, same_content};

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

/// Reconcile the existing records for a type+name (in list order) with the
/// desired records built from the `--data` values: pair them up (`Replace`),
/// `Delete` the surplus existing, `Create` the surplus desired. Pure so the
/// plan — the least-destructive way to emulate a set-replace over per-record v3
/// ops — is unit-testable.
///
/// A desired record that some existing record already holds is paired with *that*
/// record before anything is paired by position. v3 rejects a create that
/// duplicates a record still present (`DUPLICATE_RECORD`), and under a purely
/// positional pairing that is not a harmless no-op: [`super::write::apply_replace`]
/// keeps the old record when its create fails, while the surplus `Delete` later in
/// the plan still runs — so narrowing `www A {1.2.3.4, 5.6.7.8}` to just `5.6.7.8`
/// would delete `5.6.7.8` and leave `1.2.3.4` behind, the exact inverse of what was
/// asked. Pairing retained records first means one that stays in the set is never
/// re-created.
///
/// "Holds" is [`same_content`] — every field but `ttl`/`recordId`, the same test
/// `apply_replace` uses to skip a no-op — not a shared value. Records at one
/// type+name can share their value and differ in CAA `flag`/`tag`, TLSA
/// `usage`/`selector`/`matchingType`, SRV or HTTPS/SVCB fields, and pairing on the
/// value alone would pick the wrong one.
///
/// Every `existing` record must carry a `recordId`; the caller rejects the set
/// before planning otherwise.
pub(super) fn plan_set(
    existing: &[types::DnsRecord],
    desired: &[types::DnsRecord],
) -> Vec<SetAction> {
    let id = |r: &types::DnsRecord| r.record_id.clone().expect("existing record has a recordId");
    let value = |r: &types::DnsRecord| record_value(r).unwrap_or_default().to_owned();

    let mut paired: Vec<Option<usize>> = vec![None; desired.len()];
    let mut taken = vec![false; existing.len()];

    // Pass 1: pair by full content, wherever the holder sits in the list.
    for (d, want) in desired.iter().enumerate() {
        if let Some(e) =
            (0..existing.len()).find(|&e| !taken[e] && same_content(&existing[e], want))
        {
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
                record_id: id(&existing[e]),
                data: value(want),
            });
        }
    }
    // Deletes stay ahead of creates: freeing a surplus record's value is what
    // lets a create of that same value succeed.
    for (e, rec) in existing.iter().enumerate() {
        if !taken[e] {
            actions.push(SetAction::Delete { record_id: id(rec) });
        }
    }
    for (d, want) in desired.iter().enumerate() {
        if paired[d].is_none() {
            actions.push(SetAction::Create { data: value(want) });
        }
    }
    actions
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dns::records::{RecordOptions, v3_record};

    const CERT: &str = "d2abde240d7cd3ee6b4b28c54df034b97983a1d16e8a410e4561cb106618e971";

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

    /// An existing record as the API returns it: the built record plus its id.
    fn stored(id: &str, ty: &str, value: &str, opts: &RecordOptions) -> types::DnsRecord {
        types::DnsRecord {
            record_id: Some(id.to_owned()),
            ..v3_record("www", ty, value, opts)
        }
    }

    /// A desired record, built the way `dns set` builds one per `--data` value.
    fn wanted(ty: &str, value: &str, opts: &RecordOptions) -> types::DnsRecord {
        v3_record("www", ty, value, opts)
    }

    /// Existing A records from `(record_id, value)` pairs, in list order.
    fn a_stored(pairs: &[(&str, &str)]) -> Vec<types::DnsRecord> {
        pairs
            .iter()
            .map(|(id, value)| stored(id, "A", value, &opts()))
            .collect()
    }

    /// Desired A records, one per `--data` value.
    fn a_wanted(values: &[&str]) -> Vec<types::DnsRecord> {
        values
            .iter()
            .map(|value| wanted("A", value, &opts()))
            .collect()
    }

    fn tlsa_opts(usage: i64) -> RecordOptions {
        RecordOptions {
            usage: Some(usage),
            selector: Some(1),
            matching_type: Some(1),
            ..opts()
        }
    }

    fn caa_opts(tag: &str) -> RecordOptions {
        RecordOptions {
            flag: Some(0),
            tag: Some(tag.to_owned()),
            ..opts()
        }
    }

    #[test]
    fn plan_set_reuses_overlap_deletes_extra_creates_shortfall() {
        // 3 existing, 2 desired, no value in common → reuse 2 ids, delete the 3rd.
        let existing = a_stored(&[("r1", "1.1.1.1"), ("r2", "2.2.2.2"), ("r3", "3.3.3.3")]);
        let desired = a_wanted(&["9.9.9.9", "8.8.8.8"]);
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
            plan_set(&a_stored(&[("r1", "z")]), &a_wanted(&["a", "b", "c"])),
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
            plan_set(&[], &a_wanted(&["x"])),
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
        let existing = a_stored(&[("r1", "1.2.3.4"), ("r2", "5.6.7.8")]);
        assert_eq!(
            plan_set(&existing, &a_wanted(&["5.6.7.8"])),
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
        let existing = a_stored(&[("r1", "1.2.3.4"), ("r2", "5.6.7.8")]);
        assert_eq!(
            plan_set(&existing, &a_wanted(&["5.6.7.8", "1.2.3.4"])),
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
        let existing = a_stored(&[("r1", "1.2.3.4"), ("r2", "5.6.7.8")]);
        assert_eq!(
            plan_set(&existing, &a_wanted(&["5.6.7.8", "9.9.9.9"])),
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
        let existing = a_stored(&[("r1", "1.2.3.4"), ("r2", "1.2.3.4")]);
        assert_eq!(
            plan_set(&existing, &a_wanted(&["1.2.3.4", "1.2.3.4"])),
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

    #[test]
    fn plan_set_retains_the_tlsa_record_whose_usage_matches_not_the_first_with_the_same_cert() {
        // r1 and r2 share their certificate data and differ only in `usage`.
        // Asking for r2's exact content must keep r2 and drop r1. Pairing on the
        // value alone picks r1 (the first holder of the certificate): the create
        // then collides with the still-live r2, and the delete removes r2.
        let existing = [
            stored("r1", "TLSA", CERT, &tlsa_opts(3)),
            stored("r2", "TLSA", CERT, &tlsa_opts(1)),
        ];
        assert_eq!(
            plan_set(&existing, &[wanted("TLSA", CERT, &tlsa_opts(1))]),
            vec![
                SetAction::Replace {
                    record_id: "r2".into(),
                    data: CERT.into()
                },
                SetAction::Delete {
                    record_id: "r1".into()
                },
            ]
        );
    }

    #[test]
    fn plan_set_retains_the_caa_record_with_the_matching_tag_and_replaces_the_other() {
        // r1 (issue) and r2 (issuewild) both name letsencrypt.org. Keeping the
        // issuewild one while pointing `issue` at digicert.com must retain r2 as-is
        // and reuse r1 for the new value — not pair issuewild's value with r1.
        let existing = [
            stored("r1", "CAA", "letsencrypt.org", &caa_opts("issue")),
            stored("r2", "CAA", "letsencrypt.org", &caa_opts("issuewild")),
        ];
        let desired = [
            wanted("CAA", "letsencrypt.org", &caa_opts("issuewild")),
            wanted("CAA", "digicert.com", &caa_opts("issue")),
        ];
        assert_eq!(
            plan_set(&existing, &desired),
            vec![
                SetAction::Replace {
                    record_id: "r2".into(),
                    data: "letsencrypt.org".into()
                },
                SetAction::Replace {
                    record_id: "r1".into(),
                    data: "digicert.com".into()
                },
            ]
        );
    }
}
