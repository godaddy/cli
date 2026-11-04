//! Per-record write execution for `dns set`: creating, replacing and deleting
//! individual records, with the same name-exclusivity conflict handling `add`
//! gets, plus `set`'s opt-in auto-fix.

use domains_client::types;

use crate::dns::conflicts::{
    conflicting_records, conflicting_records_at, describe_duplicate_record, duplicate_record_issue,
};
use crate::dns::records::{RecordOptions, record_value, v3_record};
use crate::domain::{api_error, format_api_error};

use super::outcome::SetOutcome;

/// Context for one `set` write action, bundled to keep
/// [`write_with_conflict_handling`]'s signature within clippy's
/// argument-count limit.
#[derive(Clone, Copy)]
pub(super) struct WriteRequest<'a> {
    pub(super) domain: &'a str,
    pub(super) name: &'a str,
    pub(super) record_type: &'a str,
    pub(super) value: &'a str,
    pub(super) opts: &'a RecordOptions,
    pub(super) replace_conflicting: bool,
    /// The existing record this write replaces in place (`PUT`); `None` for a
    /// plain create (`POST`).
    pub(super) replacing: Option<&'a str>,
    pub(super) debug: bool,
}

/// Send `body` as a create, or — when `req.replacing` names a record — as an
/// in-place replace of that record.
async fn send_record(
    client: &domains_client::Client,
    req: &WriteRequest<'_>,
    body: types::DnsRecord,
) -> Result<(), Box<domains_client::Error<()>>> {
    match req.replacing {
        Some(record_id) => client
            .replace_dns_record()
            .zone(req.domain)
            .record_id(record_id)
            .body(body)
            .send()
            .await
            .map(|_| ())
            .map_err(Box::new),
        None => client
            .create_dns_record()
            .zone(req.domain)
            .body(body)
            .send()
            .await
            .map(|_| ())
            .map_err(Box::new),
    }
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

/// Write one record for `req.value` — a create for a surplus `--data` value,
/// or, when `req.replacing` is set (via [`apply_replace`]), an in-place replace
/// of that record — with the same name-exclusivity conflict handling `add` gets
/// (see [`crate::dns::conflicts::describe_write_error`]) plus an opt-in
/// auto-fix: with `--replace-conflicting-types`, a genuine `DUPLICATE_RECORD`
/// failure deletes the conflicting record(s) and retries the write once.
/// Returns the delete outcomes (if any) followed by the outcome for the write
/// itself, so `outcomes.extend(...)` folds both into the same reconcile
/// summary. The terminal outcome's `kind` is always `"created"` —
/// [`apply_replace`] relabels it to `"replaced"`.
pub(super) async fn write_with_conflict_handling(
    client: &domains_client::Client,
    req: &WriteRequest<'_>,
) -> Vec<SetOutcome> {
    let action = if req.replacing.is_some() {
        "replacing DNS record"
    } else {
        "creating DNS record"
    };
    let outcome = |err: Option<String>| SetOutcome::new("created", req.value.to_string(), err);

    let body = v3_record(req.name, req.record_type, req.value, req.opts);
    let err = match send_record(client, req, body).await {
        Ok(()) => return vec![outcome(None)],
        Err(e) => *e,
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
            req.opts,
            &at_name,
            true,
        );
        return vec![outcome(Some(msg))];
    }

    let mut outcomes = delete_conflicts(client, req.domain, &conflicts, req.debug).await;
    let retry_body = v3_record(req.name, req.record_type, req.value, req.opts);
    let retried = match send_record(client, req, retry_body).await {
        Ok(()) => outcome(None),
        Err(e) => outcome(Some(api_error(action, req.debug, *e).await.to_string())),
    };
    outcomes.push(retried);
    outcomes
}

/// Apply one `set` "replace" reconcile action: replace the record
/// `old_record_id` in place with the desired value via a single `PUT`. Only
/// called for a pairing that actually changes something —
/// [`super::plan::plan_set`] has already set aside the ones that don't as
/// `Unchanged`.
///
/// A `PUT` must carry a TTL. When the caller gave no `--ttl`, the record keeps
/// the TTL it has (`old_ttl`) rather than being reset to the default, the same
/// as a record `set` leaves alone.
///
/// Relabels the write outcome's `kind` from `"created"` to `"replaced"` so
/// `summarize_set_outcomes`'s tallies mean what the user asked for.
pub(super) async fn apply_replace(
    client: &domains_client::Client,
    req: &WriteRequest<'_>,
    old_record_id: &str,
    old_ttl: Option<i64>,
) -> Vec<SetOutcome> {
    let mut opts = req.opts.clone();
    opts.ttl = opts.ttl.or(old_ttl);
    let req = WriteRequest {
        opts: &opts,
        replacing: Some(old_record_id),
        ..*req
    };
    let mut outcomes = write_with_conflict_handling(client, &req).await;
    if let Some(last) = outcomes.last_mut() {
        last.kind = "replaced";
    }
    outcomes
}

#[cfg(test)]
#[path = "write_tests.rs"]
mod tests;
