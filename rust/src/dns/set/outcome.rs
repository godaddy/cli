//! Reporting for `dns set`: tallying applied reconcile outcomes into the
//! command's success/failure result, and building the `--dry-run` preview
//! directly from a reconcile plan. Both report the same `records` (see
//! [`change_entry`]), so a script reads a preview and a real run the same way.

use domains_client::types;
use serde_json::{Value, json};

use super::super::records::{RecordOptions, change_entry, record_value, v3_record};
use super::plan::SetAction;

/// The `action` of a `set` that has nothing to do, in a preview and a real run alike.
const NOTHING_TO_CHANGE: &str = "nothing to change";

/// Outcome of one applied `set` reconcile action, for reporting.
pub(super) struct SetOutcome {
    /// What was attempted: `"replaced"`, `"unchanged"`, `"created"`, or `"deleted"`.
    pub(super) kind: &'static str,
    /// The `--data` value (replace/create) or record id (delete).
    pub(super) detail: String,
    /// `Some(msg)` if the call failed.
    pub(super) error: Option<String>,
    /// The record's id: the one replaced, left alone or deleted. A created record
    /// has it in `new`.
    pub(super) record_id: Option<String>,
    /// The record as it was before the action; `None` for a create.
    pub(super) old: Option<types::DnsRecord>,
    /// The record as it is after the action, as the API returned it; `None` for a
    /// delete.
    pub(super) new: Option<types::DnsRecord>,
}

impl SetOutcome {
    pub(super) fn new(kind: &'static str, detail: String, error: Option<String>) -> Self {
        Self {
            kind,
            detail,
            error,
            record_id: None,
            old: None,
            new: None,
        }
    }

    /// A successful write, with the record the API sent back.
    pub(super) fn written(kind: &'static str, detail: String, record: types::DnsRecord) -> Self {
        Self {
            record_id: record.record_id.clone(),
            new: Some(record),
            ..Self::new(kind, detail, None)
        }
    }

    /// This outcome's entry in the result's `records`.
    fn change_entry(&self) -> Value {
        let action = match self.kind {
            "replaced" => "replace",
            "created" => "create",
            "deleted" => "delete",
            _ => "unchanged",
        };
        let record_id = self
            .record_id
            .as_deref()
            .or_else(|| self.new.as_ref().and_then(|r| r.record_id.as_deref()));
        change_entry(action, record_id, self.old.as_ref(), self.new.as_ref())
    }
}

/// The `set` result: the counts, the `records` showing what was (or would be) done to each
/// record, and an `action` that says `nothing to change` when no record is touched.
fn set_result(
    domain: &str,
    record_type: &str,
    name: &str,
    [replaced, unchanged, created, deleted]: [usize; 4],
    action: &str,
    records: Vec<Value>,
) -> Value {
    json!({
        "domain": domain,
        "type": record_type,
        "name": name,
        "replaced": replaced,
        "unchanged": unchanged,
        "created": created,
        "deleted": deleted,
        "action": if replaced + created + deleted == 0 { NOTHING_TO_CHANGE } else { action },
        "records": records,
    })
}

/// Build the `set` result from the applied reconcile outcomes: `Ok(json)` with
/// replaced/unchanged/created/deleted counts and the executed `records` when every
/// action succeeded, or `Err(message)` — a non-zero exit — with a per-action ✓/✗
/// breakdown if any failed. Pure so the success/failure decision and tallies are
/// unit-testable.
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
    let counts = [
        count("replaced"),
        count("unchanged"),
        count("created"),
        count("deleted"),
    ];

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

    let records = outcomes.iter().map(SetOutcome::change_entry).collect();
    Ok(set_result(
        domain,
        record_type,
        name,
        counts,
        "set",
        records,
    ))
}

/// Builds the `dns set --dry-run` preview directly from the same reconcile
/// plan the real execution would apply, so the preview can never drift from
/// what `set` would actually do. Each entry shows the record as it is now (`old`)
/// and as the write would leave it (`new`). Doesn't attempt to predict a
/// name-exclusivity conflict (that's only known once a write is actually
/// attempted) — the preview shows the reconcile plan, not whether
/// `--replace-conflicting-types` would end up mattering.
pub(super) fn dry_run_set_preview(
    domain: &str,
    record_type: &str,
    name: &str,
    plan: &[SetAction],
    existing: &[types::DnsRecord],
    opts: &RecordOptions,
) -> Value {
    let count = |predicate: fn(&SetAction) -> bool| plan.iter().filter(|a| predicate(a)).count();
    let stored = |record_id: &str| {
        existing
            .iter()
            .find(|r| r.record_id.as_deref() == Some(record_id))
    };
    // The record a write of `value` would leave: what `--data` and the flags
    // describe, kept at the old record's TTL when none was given (as the real
    // replace does).
    let wanted = |value: &str, replacing: Option<&types::DnsRecord>| types::DnsRecord {
        record_id: replacing.and_then(|r| r.record_id.clone()),
        ..v3_record(
            name,
            record_type,
            value,
            &RecordOptions {
                ttl: opts.ttl.or(replacing.map(|r| r.ttl)),
                ..opts.clone()
            },
        )
    };
    let records: Vec<Value> = plan
        .iter()
        .map(|action| match action {
            SetAction::Replace { record_id, data } => {
                let old = stored(record_id);
                change_entry("replace", Some(record_id), old, Some(&wanted(data, old)))
            }
            SetAction::Unchanged { record_id, .. } => {
                let old = stored(record_id);
                change_entry("unchanged", Some(record_id), old, old)
            }
            SetAction::Create { data } => {
                change_entry("create", None, None, Some(&wanted(data, None)))
            }
            SetAction::Delete { record_id } => {
                change_entry("delete", Some(record_id), stored(record_id), None)
            }
        })
        .collect();

    set_result(
        domain,
        record_type,
        name,
        [
            count(|a| matches!(a, SetAction::Replace { .. })),
            count(|a| matches!(a, SetAction::Unchanged { .. })),
            count(|a| matches!(a, SetAction::Create { .. })),
            count(|a| matches!(a, SetAction::Delete { .. })),
        ],
        "would set",
        records,
    )
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
        .map(|r| format!("{record_type} {}", record_value(r).unwrap_or("(no data)")))
        .unwrap_or_else(|| record_id.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tlsa_record(record_id: &str, cert: &str) -> types::DnsRecord {
        types::DnsRecord {
            certificate_data: Some(cert.to_owned()),
            matching_type: None,
            selector: None,
            usage: None,
            data: None,
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
            type_: types::DnsRecordType("TLSA".to_owned()),
            weight: None,
        }
    }

    /// TLSA's value lives in `certificate_data`, not `data` — the label must
    /// read through `record_value` to show it rather than "(no data)".
    #[test]
    fn record_label_shows_tlsa_certificate_data_as_the_value() {
        let cert = "d2abde240d7cd3ee6b4b28c54df034b97983a1d16e8a410e4561cb106618e971";
        let existing = vec![tlsa_record("r1", cert)];
        assert_eq!(
            record_label(&existing, "TLSA", "r1"),
            format!("TLSA {cert}")
        );
    }

    #[test]
    fn summarize_set_reports_counts_and_flags_partial_failure() {
        let all_ok = vec![
            SetOutcome::new("replaced", "9.9.9.9".into(), None),
            SetOutcome::new("unchanged", "7.7.7.7".into(), None),
            SetOutcome::new("created", "8.8.8.8".into(), None),
            SetOutcome::new("deleted", "r3".into(), None),
        ];
        let v = summarize_set_outcomes("example.com", "A", "www", &all_ok).expect("all ok");
        assert_eq!(v["replaced"], 1);
        assert_eq!(v["unchanged"], 1);
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

    /// An existing A record at `www`, as the API returns it.
    fn stored(id: &str, value: &str) -> types::DnsRecord {
        types::DnsRecord {
            record_id: Some(id.to_owned()),
            ..v3_record("www", "A", value, &opts())
        }
    }

    fn replace(id: &str, data: &str) -> SetAction {
        SetAction::Replace {
            record_id: id.into(),
            data: data.into(),
        }
    }

    fn unchanged(id: &str, data: &str) -> SetAction {
        SetAction::Unchanged {
            record_id: id.into(),
            data: data.into(),
        }
    }

    fn preview(plan: &[SetAction], existing: &[types::DnsRecord]) -> Value {
        dry_run_set_preview("example.com", "A", "www", plan, existing, &opts())
    }

    #[test]
    fn dry_run_set_preview_matches_the_plan_it_would_execute() {
        let existing = [
            stored("r1", "1.1.1.1"),
            stored("r2", "8.8.8.8"),
            stored("r3", "3.3.3.3"),
        ];
        let plan = [
            replace("r1", "9.9.9.9"),
            unchanged("r2", "8.8.8.8"),
            SetAction::Delete {
                record_id: "r3".into(),
            },
        ];
        let preview = preview(&plan, &existing);
        assert_eq!(preview["replaced"], 1);
        assert_eq!(preview["unchanged"], 1);
        assert_eq!(preview["created"], 0);
        assert_eq!(preview["deleted"], 1);
        assert_eq!(preview["action"], "would set");
        assert_eq!(
            preview["records"].as_array().expect("records array").len(),
            3
        );
    }

    /// The ticket's headline gap: a replace has to show the value it replaces, not
    /// just the one it writes — and a delete the value it removes.
    #[test]
    fn the_preview_shows_the_old_value_the_new_value_and_the_id_of_every_entry() {
        let existing = [stored("r1", "1.1.1.1"), stored("r2", "3.3.3.3")];
        let plan = [
            replace("r1", "9.9.9.9"),
            SetAction::Delete {
                record_id: "r2".into(),
            },
            SetAction::Create {
                data: "8.8.8.8".into(),
            },
        ];
        let preview = preview(&plan, &existing);
        let records = preview["records"].as_array().expect("records array");

        assert_eq!(records[0]["action"], "replace");
        assert_eq!(records[0]["recordId"], "r1");
        assert_eq!(records[0]["old"]["data"], "1.1.1.1");
        assert_eq!(records[0]["new"]["data"], "9.9.9.9");

        assert_eq!(records[1]["action"], "delete");
        assert_eq!(records[1]["recordId"], "r2");
        assert_eq!(records[1]["old"]["data"], "3.3.3.3");
        assert_eq!(records[1]["new"], Value::Null);

        assert_eq!(records[2]["action"], "create");
        assert_eq!(records[2]["recordId"], Value::Null);
        assert_eq!(records[2]["old"], Value::Null);
        assert_eq!(records[2]["new"]["data"], "8.8.8.8");
    }

    /// A replace keeps the old record's TTL unless `--ttl` is given, as the real
    /// `PUT` does — so the preview's `new` is the record the write would leave.
    #[test]
    fn the_preview_keeps_the_stored_ttl_unless_one_was_asked_for() {
        let slow = types::DnsRecord {
            ttl: 600,
            ..stored("r1", "1.1.1.1")
        };
        let plan = [replace("r1", "9.9.9.9")];

        let kept = preview(&plan, std::slice::from_ref(&slow));
        assert_eq!(kept["records"][0]["old"]["ttl"], 600);
        assert_eq!(kept["records"][0]["new"]["ttl"], 600);

        let asked = RecordOptions {
            ttl: Some(900),
            ..opts()
        };
        let changed = dry_run_set_preview("example.com", "A", "www", &plan, &[slow], &asked);
        assert_eq!(changed["records"][0]["new"]["ttl"], 900);
    }

    /// The ticket's no-op case: setting what is already there is `unchanged`, not
    /// a replacement, and both the preview and the real run say so in the same words.
    #[test]
    fn a_no_op_set_reports_nothing_to_change_in_a_preview_and_a_real_run() {
        let existing = [stored("r1", "1.1.1.1"), stored("r2", "2.2.2.2")];
        let plan = [unchanged("r1", "1.1.1.1"), unchanged("r2", "2.2.2.2")];
        let previewed = preview(&plan, &existing);

        let outcomes: Vec<SetOutcome> = existing
            .iter()
            .map(|record| {
                let mut o = SetOutcome::new("unchanged", "x".into(), None);
                o.record_id = record.record_id.clone();
                o.old = Some(record.clone());
                o.new = Some(record.clone());
                o
            })
            .collect();
        let done = summarize_set_outcomes("example.com", "A", "www", &outcomes).expect("ok");

        for result in [&previewed, &done] {
            assert_eq!(result["replaced"], 0);
            assert_eq!(result["unchanged"], 2);
            assert_eq!(result["action"], "nothing to change");
        }
        assert_eq!(previewed["records"], done["records"]);
    }

    /// Run for real, a `set` reports the same records its preview did: the same
    /// actions, old and new values and ids.
    #[test]
    fn a_real_set_reports_the_same_records_as_its_preview() {
        let existing = [stored("r1", "1.1.1.1"), stored("r2", "3.3.3.3")];
        let plan = [
            replace("r1", "9.9.9.9"),
            SetAction::Delete {
                record_id: "r2".into(),
            },
        ];
        let previewed = preview(&plan, &existing);

        let mut replaced = SetOutcome::written(
            "replaced",
            "9.9.9.9".into(),
            types::DnsRecord {
                record_id: Some("r1".into()),
                ..v3_record("www", "A", "9.9.9.9", &opts())
            },
        );
        replaced.old = Some(existing[0].clone());
        let mut deleted = SetOutcome::new("deleted", "A 3.3.3.3".into(), None);
        deleted.record_id = Some("r2".into());
        deleted.old = Some(existing[1].clone());
        let done =
            summarize_set_outcomes("example.com", "A", "www", &[replaced, deleted]).expect("ok");

        assert_eq!(previewed["records"], done["records"]);
        assert_eq!(done["action"], "set");
        assert_eq!(done["replaced"], 1);
        assert_eq!(done["deleted"], 1);
    }

    /// A create made for real has no id in the preview; the result gives
    /// the id the API assigned.
    #[test]
    fn a_real_create_reports_the_id_the_api_assigned() {
        let made = SetOutcome::written(
            "created",
            "8.8.8.8".into(),
            types::DnsRecord {
                record_id: Some("new-1".into()),
                ..v3_record("www", "A", "8.8.8.8", &opts())
            },
        );
        let done = summarize_set_outcomes("example.com", "A", "www", &[made]).expect("ok");
        assert_eq!(done["records"][0]["action"], "create");
        assert_eq!(done["records"][0]["recordId"], "new-1");
        assert_eq!(done["records"][0]["new"]["data"], "8.8.8.8");
    }

    /// The command's `--output json` path always projects through
    /// `default_fields` when the user doesn't pass `--fields` — a preview
    /// field that isn't in that list is silently stripped and never reaches
    /// the user. Prove the preview's summary fields actually survive it
    /// (this is exactly the gap a pure call to `dry_run_set_preview` can't
    /// catch, since it never goes through the projection).
    #[test]
    fn dry_run_set_preview_survives_default_field_projection() {
        let existing = [stored("r1", "1.1.1.1")];
        let preview = preview(&[replace("r1", "9.9.9.9")], &existing);
        let projected = cli_engine::output::filter_fields(&preview, super::super::DEFAULT_FIELDS);
        for field in [
            "domain",
            "type",
            "name",
            "replaced",
            "unchanged",
            "created",
            "deleted",
            "action",
            "records",
        ] {
            assert!(
                !projected[field].is_null(),
                "{field:?} was stripped by default_fields; preview: {projected}"
            );
        }
    }

    /// Proves `view_columns()`'s field names actually match what
    /// `dry_run_set_preview` emits — a mismatch here would silently drop the
    /// per-record changes from human output.
    #[test]
    fn dry_run_set_preview_renders_records_as_a_nested_table() {
        let existing = [stored("r1", "1.1.1.1")];
        let plan = [
            replace("r1", "9.9.9.9"),
            SetAction::Create {
                data: "8.8.8.8".into(),
            },
        ];
        let rendered = cli_engine::preview_human_view(
            preview(&plan, &existing),
            &super::super::view_columns(),
        );
        assert!(rendered.contains("Records:"), "{rendered}");
        assert!(rendered.contains("RECORD ID"), "{rendered}");
        assert!(rendered.contains("replace"), "{rendered}");
        assert!(rendered.contains("create"), "{rendered}");
        assert!(rendered.contains("1.1.1.1"), "{rendered}");
        assert!(rendered.contains("9.9.9.9"), "{rendered}");
    }

    /// The records list what was done to SRV records told apart by service and
    /// port, not just by their identical data — and adds nothing for plain ones.
    #[test]
    fn the_records_show_srv_service_and_port_and_nothing_extra_for_a_records() {
        let columns = super::super::view_columns();
        let srv = |id: &str, service: &str, port: u16| types::DnsRecord {
            record_id: Some(id.to_owned()),
            ..v3_record(
                "chat",
                "SRV",
                "t.example.com.",
                &RecordOptions {
                    priority: Some(10),
                    weight: Some(5),
                    port: Some(i64::from(port)),
                    service: Some(service.to_owned()),
                    protocol: Some("_tcp".to_owned()),
                    ..opts()
                },
            )
        };
        let existing = [srv("r1", "_sip", 5060), srv("r2", "_xmpp", 5222)];
        let plan = [
            SetAction::Delete {
                record_id: "r1".into(),
            },
            SetAction::Delete {
                record_id: "r2".into(),
            },
        ];
        let result = dry_run_set_preview("example.com", "SRV", "chat", &plan, &existing, &opts());
        let rendered = cli_engine::preview_human_view(result, &columns);
        assert!(
            rendered.contains("SERVICE") && rendered.contains("PORT"),
            "{rendered}"
        );
        assert!(
            rendered.contains("_sip") && rendered.contains("_xmpp"),
            "{rendered}"
        );

        let rendered = cli_engine::preview_human_view(
            preview(&[unchanged("r1", "1.1.1.1")], &[stored("r1", "1.1.1.1")]),
            &columns,
        );
        for sparse in ["SERVICE", "PORT", "PRIORITY", "PROTOCOL"] {
            assert!(!rendered.contains(sparse), "{sparse} shown: {rendered}");
        }
    }
}
