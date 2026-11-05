//! Per-record write execution for `dns set`: creating/deleting individual
//! records, with the same name-exclusivity conflict handling `add` gets, plus
//! `set`'s opt-in auto-fix and create-then-delete replace semantics.

use domains_client::types;

use crate::dns::conflicts::{
    conflicting_records, conflicting_records_at, describe_duplicate_record, duplicate_record_issue,
};
use crate::dns::records::{RecordOptions, record_value, same_content, v3_record};
use crate::domain::{api_error, format_api_error};

use super::outcome::SetOutcome;

/// Context for one `set` create action, bundled to keep
/// [`write_with_conflict_handling`]'s signature within clippy's
/// argument-count limit.
pub(super) struct WriteRequest<'a> {
    pub(super) domain: &'a str,
    pub(super) name: &'a str,
    pub(super) record_type: &'a str,
    pub(super) value: &'a str,
    pub(super) opts: &'a RecordOptions,
    pub(super) replace_conflicting: bool,
    pub(super) debug: bool,
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
        let detail = format!(
            "{} {}",
            rec.type_.as_str(),
            record_value(rec).unwrap_or("(no data)")
        );
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

/// Create one record for `req.value` (a surplus `--data` value, or — via
/// [`apply_replace`] — the new half of a replace pairing), with the same
/// name-exclusivity conflict handling `add` gets (see
/// [`crate::dns::conflicts::describe_write_error`]) plus an opt-in auto-fix: with
/// `--replace-conflicting-types`, a genuine `DUPLICATE_RECORD` failure deletes
/// the conflicting record(s) and retries the write once. Returns the delete
/// outcomes (if any) followed by the outcome for the create itself, so
/// `outcomes.extend(...)` folds both into the same reconcile summary. The
/// terminal outcome's `kind` is always `"created"` — [`apply_replace`]
/// relabels it to `"replaced"` when this is the create half of a replace.
pub(super) async fn write_with_conflict_handling(
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
            req.opts,
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

/// Apply one `set` "replace" reconcile action: create the new value via
/// [`write_with_conflict_handling`], then — only if that succeeded — delete
/// the record it's replacing. There's no atomic single-record replace in v3
/// (see the note on `SetAction::Replace`), so this create-then-delete pair
/// is the least-destructive way to emulate one: if the create fails, the old
/// record is left untouched rather than risking data loss.
///
/// `old_record` is the record being replaced, if known — when its content
/// already matches the desired record (a no-op `set`, e.g. re-running the
/// same command, or only some of several values actually changed) this is a
/// no-op: creating the "new" value while the identical old record still
/// exists would fail with v3's exact-duplicate `DUPLICATE_RECORD` (see
/// [`crate::dns::conflicts::describe_duplicate_record`]), which isn't a real
/// failure, just this pairing having nothing to do. Compares full record
/// content ([`same_content`]), not just the value — a record type whose
/// identity spans several fields (CAA's `flag`/`tag`, TLSA's `usage`/
/// `selector`/`matchingType`, …) could otherwise have a field-only change
/// wrongly skipped as a no-op. `same_content` deliberately ignores `ttl`, so
/// an explicit `--ttl` change (checked separately here, against the *raw*
/// `req.opts.ttl` rather than `desired.ttl`'s defaulted value, so an omitted
/// `--ttl` never forces a replace just because the default differs from the
/// existing record's) still counts as a real change even when the value and
/// every other field are unchanged — but that one can't be create-then-delete
/// (the create would be an exact duplicate of the record it replaces), so it
/// goes through [`retime_record`] instead.
///
/// Relabels the create outcome's `kind` from `"created"` to `"replaced"` so
/// `summarize_set_outcomes`'s tallies mean what the user asked for. If the
/// delete of the old record fails after a successful create, that's reported
/// as an extra `"cleanup"`-kind outcome — a kind `summarize_set_outcomes`'s
/// counts don't recognize, so it doesn't inflate replaced/created/deleted,
/// but its `error` still fails the command and shows up in the breakdown.
pub(super) async fn apply_replace(
    client: &domains_client::Client,
    req: &WriteRequest<'_>,
    old_record_id: &str,
    old_detail: &str,
    old_record: Option<&types::DnsRecord>,
) -> Vec<SetOutcome> {
    let desired = v3_record(req.name, req.record_type, req.value, req.opts);
    if let Some(old) = old_record.filter(|r| same_content(r, &desired)) {
        // Same record; only an explicit `--ttl` (not the defaulted one on
        // `desired`) that differs from the stored one is a change.
        return match req.opts.ttl.filter(|&t| t != old.ttl) {
            Some(_) => retime_record(client, req, old_record_id, old_detail, old).await,
            None => vec![SetOutcome::new("replaced", req.value.to_string(), None)],
        };
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

/// Change only the TTL of a record `set` is keeping. v3 identifies a record by
/// its content, not its TTL, so creating the new-TTL record while the old one
/// still exists fails as an exact duplicate — the old record has to be deleted
/// first. That leaves the value missing until the create lands, so if the create
/// fails the old record is put back unchanged and the failure reported; only if
/// that restore fails too is a `"cleanup"` outcome added saying the record is gone.
async fn retime_record(
    client: &domains_client::Client,
    req: &WriteRequest<'_>,
    old_record_id: &str,
    old_detail: &str,
    old: &types::DnsRecord,
) -> Vec<SetOutcome> {
    let replaced = |err: Option<String>| SetOutcome::new("replaced", req.value.to_string(), err);

    if let Err(e) = client
        .delete_dns_record()
        .zone(req.domain)
        .record_id(old_record_id)
        .send()
        .await
    {
        return vec![replaced(Some(
            api_error("deleting the previous DNS record", req.debug, e)
                .await
                .to_string(),
        ))];
    }

    let mut outcomes = write_with_conflict_handling(client, req).await;
    let Some(last) = outcomes.last_mut() else {
        return outcomes;
    };
    last.kind = "replaced";
    let Some(create_err) = last.error.clone() else {
        return outcomes;
    };

    let restored = types::DnsRecord {
        record_id: None,
        ..old.clone()
    };
    match client
        .create_dns_record()
        .zone(req.domain)
        .body(restored)
        .send()
        .await
    {
        Ok(_) => {
            last.error = Some(format!(
                "{create_err}; the previous record was left as it was"
            ))
        }
        Err(e) => {
            let msg = format!(
                "the previous record ({old_detail}) was removed to change its TTL, the new one \
                 could not be created, and putting it back failed: {}; run `gddy dns list {} \
                 --type {} --name {}` and re-create it with `gddy dns add` if it's gone",
                api_error("restoring the previous DNS record", req.debug, e).await,
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
    }
    outcomes
}

#[cfg(test)]
#[path = "write_tests.rs"]
mod tests;
