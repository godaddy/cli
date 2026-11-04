//! Reconcile planning for `dns set` — deciding, from the existing records
//! and desired `--data` values, which per-record ops to run.

use domains_client::types;

use crate::dns::records::{record_value, same_content};

/// One reconcile action for `dns set` over the API's per-record endpoints.
#[derive(Debug, PartialEq)]
pub(super) enum SetAction {
    /// Replace the existing record (`record_id`) in place with the desired
    /// `--data` value.
    Replace { record_id: String, data: String },
    /// The existing record (`record_id`) already is the desired one; nothing to
    /// send.
    Unchanged { record_id: String, data: String },
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
pub(super) fn plan_set(
    existing: &[types::DnsRecord],
    desired: &[types::DnsRecord],
    ttl: Option<i64>,
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
            let record_id = id(&existing[e]);
            let data = value(want);
            let unchanged =
                same_content(&existing[e], want) && ttl.is_none_or(|t| t == existing[e].ttl);
            actions.push(if unchanged {
                SetAction::Unchanged { record_id, data }
            } else {
                SetAction::Replace { record_id, data }
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
            plan_set(&existing, &desired, None),
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
            plan_set(&a_stored(&[("r1", "z")]), &a_wanted(&["a", "b", "c"]), None),
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
            plan_set(&[], &a_wanted(&["x"]), None),
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
            plan_set(&existing, &a_wanted(&["5.6.7.8"]), None),
            vec![
                SetAction::Unchanged {
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
            plan_set(&existing, &a_wanted(&["5.6.7.8", "1.2.3.4"]), None),
            vec![
                SetAction::Unchanged {
                    record_id: "r2".into(),
                    data: "5.6.7.8".into()
                },
                SetAction::Unchanged {
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
            plan_set(&existing, &a_wanted(&["5.6.7.8", "9.9.9.9"]), None),
            vec![
                SetAction::Unchanged {
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
            plan_set(&existing, &a_wanted(&["1.2.3.4", "1.2.3.4"]), None),
            vec![
                SetAction::Unchanged {
                    record_id: "r1".into(),
                    data: "1.2.3.4".into()
                },
                SetAction::Unchanged {
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
            plan_set(&existing, &[wanted("TLSA", CERT, &tlsa_opts(1))], None),
            vec![
                SetAction::Unchanged {
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
            plan_set(&existing, &desired, None),
            vec![
                SetAction::Unchanged {
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

    #[test]
    fn plan_set_ignores_a_ttl_the_caller_did_not_pass() {
        // The stored record has ttl 600; `a_wanted` builds with the 3600 default.
        // Without an explicit --ttl that difference is not a change.
        let mut slow = opts();
        slow.ttl = Some(600);
        let existing = [stored("r1", "A", "1.2.3.4", &slow)];
        assert_eq!(
            plan_set(&existing, &a_wanted(&["1.2.3.4"]), None),
            vec![SetAction::Unchanged {
                record_id: "r1".into(),
                data: "1.2.3.4".into()
            }]
        );
    }

    #[test]
    fn plan_set_replaces_when_an_explicit_ttl_differs() {
        let existing = a_stored(&[("r1", "1.2.3.4")]); // stored at the 3600 default
        let mut fast = opts();
        fast.ttl = Some(900);
        let desired = [wanted("A", "1.2.3.4", &fast)];
        assert_eq!(
            plan_set(&existing, &desired, Some(900)),
            vec![SetAction::Replace {
                record_id: "r1".into(),
                data: "1.2.3.4".into()
            }]
        );
        // The same explicit ttl as stored is no change.
        assert_eq!(
            plan_set(&existing, &desired, Some(3600)),
            vec![SetAction::Unchanged {
                record_id: "r1".into(),
                data: "1.2.3.4".into()
            }]
        );
    }

    #[test]
    fn plan_set_treats_an_api_normalized_record_as_unchanged() {
        // `Example.COM` is stored as `example.com.`; asking for it again is a
        // no-op, not another replace.
        let existing = [stored("r1", "CNAME", "example.com.", &opts())];
        let desired = [wanted("CNAME", "Example.COM", &opts())];
        assert_eq!(
            plan_set(&existing, &desired, None),
            vec![SetAction::Unchanged {
                record_id: "r1".into(),
                data: "Example.COM".into()
            }]
        );
    }

    fn srv_opts() -> RecordOptions {
        RecordOptions {
            priority: Some(1),
            weight: Some(1),
            port: Some(5222),
            service: Some("_xmpp".to_owned()),
            protocol: Some("_tcp".to_owned()),
            ..opts()
        }
    }

    #[test]
    fn plan_set_keeps_the_srv_target_that_was_asked_for_when_the_api_added_a_trailing_dot() {
        // The API stores SRV targets as `a.example.com.` and `b.example.com.`.
        // Asking for just `b.example.com` (no dot) must pair with r2, which already
        // holds it, and delete r1.
        // Compared verbatim, nothing matches, r1 is paired with the request by
        // position, its write fails as a duplicate of r2, and the surplus delete
        // then removes r2 — leaving only the target that was meant to go.
        let o = srv_opts();
        let existing = [
            stored("r1", "SRV", "a.example.com.", &o),
            stored("r2", "SRV", "b.example.com.", &o),
        ];
        assert_eq!(
            plan_set(&existing, &[wanted("SRV", "b.example.com", &o)], None),
            vec![
                SetAction::Unchanged {
                    record_id: "r2".into(),
                    data: "b.example.com".into()
                },
                SetAction::Delete {
                    record_id: "r1".into()
                },
            ]
        );
    }
}
