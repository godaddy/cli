//! Executing a reconcile plan: the per-record writes and deletes `dns set`
//! makes, shared with `dns import`, which builds plans of its own.

use domains_client::types;

use crate::dns::failure::DnsFailure;
use crate::dns::records::RecordOptions;

use super::outcome::{SetOutcome, record_label};
use super::plan::SetAction;
use super::write::{WriteRequest, apply_replace, write_with_conflict_handling};

/// What a plan is applied to.
pub(in crate::dns) struct PlanTarget<'a> {
    pub(in crate::dns) domain: &'a str,
    pub(in crate::dns) name: &'a str,
    pub(in crate::dns) record_type: &'a str,
    /// The records the plan was made from; a replace or delete looks its record
    /// up here to report what it changed.
    pub(in crate::dns) existing: &'a [types::DnsRecord],
    /// The options of each desired record, indexed by a plan action's `desired`.
    pub(in crate::dns) opts: &'a [RecordOptions],
    /// Remove a record that blocks a write (a CNAME against other types) and retry.
    pub(in crate::dns) replace_conflicting: bool,
    pub(in crate::dns) debug: bool,
}

/// The write of `value`, the desired record at position `at`.
fn request<'a>(target: &PlanTarget<'a>, value: &'a str, at: usize) -> WriteRequest<'a> {
    WriteRequest {
        domain: target.domain,
        name: target.name,
        record_type: target.record_type,
        value,
        opts: &target.opts[at],
        replace_conflicting: target.replace_conflicting,
        replacing: None,
        debug: target.debug,
    }
}

/// Run every action of `plan` in order, never stopping at a failure, and report
/// what happened to each. The caller decides what a failure means (see
/// `summarize_set_outcomes`).
pub(in crate::dns) async fn apply_plan(
    client: &domains_client::Client,
    target: &PlanTarget<'_>,
    plan: Vec<SetAction>,
) -> Vec<SetOutcome> {
    let stored = |record_id: &str| {
        target
            .existing
            .iter()
            .find(|r| r.record_id.as_deref() == Some(record_id))
    };
    let mut outcomes = Vec::new();
    for action in plan {
        match action {
            SetAction::Replace {
                record_id,
                data: value,
                desired: at,
            } => {
                let req = request(target, &value, at);
                let old = stored(&record_id);
                let mut written = apply_replace(client, &req, &record_id, old.map(|r| r.ttl)).await;
                // The last outcome is the replace itself; any before it
                // removed a conflicting record.
                if let Some(last) = written.last_mut() {
                    last.record_id = Some(record_id.clone());
                    last.old = old.cloned();
                }
                outcomes.extend(written);
            }
            SetAction::Unchanged {
                data: value,
                record_id,
                ..
            } => {
                let old = stored(&record_id);
                let mut outcome = SetOutcome::new("unchanged", value, None);
                outcome.record_id = Some(record_id);
                outcome.old = old.cloned();
                outcome.new = old.cloned();
                outcomes.push(outcome);
            }
            SetAction::Create {
                data: value,
                desired: at,
            } => {
                let req = request(target, &value, at);
                outcomes.extend(write_with_conflict_handling(client, &req).await);
            }
            SetAction::Delete { record_id } => {
                let res = client
                    .delete_dns_record()
                    .zone(target.domain)
                    .record_id(record_id.as_str())
                    .send()
                    .await;
                let err = match res {
                    Ok(_) => None,
                    Err(e) => {
                        Some(DnsFailure::from_error("deleting DNS record", target.debug, e).await)
                    }
                };
                let detail = record_label(target.existing, target.record_type, &record_id);
                let mut outcome = SetOutcome::new("deleted", detail, err);
                outcome.old = stored(&record_id).cloned();
                outcome.record_id = Some(record_id);
                outcomes.push(outcome);
            }
        }
    }
    outcomes
}
