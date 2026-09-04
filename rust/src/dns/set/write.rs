//! Per-record write execution for `dns set`: creating/deleting individual
//! records, with the same name-exclusivity conflict handling `add` gets, plus
//! `set`'s opt-in auto-fix and create-then-delete replace semantics.

use domains_client::types;

use crate::dns::conflicts::{
    conflicting_records, conflicting_records_at, describe_duplicate_record, duplicate_record_issue,
};
use crate::dns::records::{RecordOptions, v3_record};
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
            rec.data.as_deref().unwrap_or("(no data)")
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
/// `old_data` is the record's *current* value, if known — when it already
/// equals `req.value` (a no-op `set`, e.g. re-running the same command, or
/// only some of several values actually changed) this is a no-op: creating
/// the "new" value while the identical old record still exists would fail
/// with v3's exact-duplicate `DUPLICATE_RECORD` (see
/// [`crate::dns::conflicts::describe_duplicate_record`]), which isn't a real
/// failure, just this pairing having nothing to do.
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    // --- write_with_conflict_handling ---------------------------------------
    //
    // These exercise the DUPLICATE_RECORD branching against a mock server: the
    // no-conflict path, the conflict-without-`--replace-conflicting-types`
    // path (message only, no delete attempted), and the conflict-with-the-flag
    // path (delete_conflicts outcome then the retry outcome, in that order).

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
            usage: None,
            selector: None,
            matching_type: None,
            parameters: None,
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
    async fn write_with_conflict_handling_sends_https_parameters_in_request_body() {
        let server = MockServer::start_async().await;
        let create = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v3/domains/zones/example.com/dns-records")
                    .json_body(json!({
                        "data": ".",
                        "name": "@",
                        "parameters": "alpn=h2,h3",
                        "priority": 1,
                        "ttl": 3600,
                        "type": "HTTPS"
                    }));
                then.status(201)
                    .json_body(json!({ "type": "HTTPS", "name": "@", "data": ".", "ttl": 3600 }));
            })
            .await;

        let mut opts = write_opts();
        opts.priority = Some(1);
        opts.parameters = Some("alpn=h2,h3".to_string());
        let req = WriteRequest {
            domain: "example.com",
            name: "@",
            record_type: "HTTPS",
            value: ".",
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
    async fn write_with_conflict_handling_sends_tlsa_fields_in_request_body() {
        let server = MockServer::start_async().await;
        let cert = "d2abde240d7cd3ee6b4b28c54df034b97983a1d16e8a410e4561cb106618e971";
        let create = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v3/domains/zones/example.com/dns-records")
                    .json_body(json!({
                        "certificateData": cert,
                        "matchingType": 1,
                        "name": "www",
                        "port": 443,
                        "protocol": "_tcp",
                        "selector": 1,
                        "ttl": 3600,
                        "type": "TLSA",
                        "usage": 3
                    }));
                then.status(201)
                    .json_body(json!({ "type": "TLSA", "name": "www", "ttl": 3600 }));
            })
            .await;

        let mut opts = write_opts();
        opts.usage = Some(3);
        opts.selector = Some(1);
        opts.matching_type = Some(1);
        opts.protocol = Some("_tcp".to_string());
        opts.port = Some(443);
        let req = WriteRequest {
            domain: "example.com",
            name: "www",
            record_type: "TLSA",
            value: cert,
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
            create.calls_async().await,
            0,
            "no create for a no-op replace"
        );
        assert_eq!(
            delete.calls_async().await,
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
            delete.calls_async().await,
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
