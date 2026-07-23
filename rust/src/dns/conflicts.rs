//! DNS name-exclusivity conflict diagnosis, shared by `add` and `set`: DNS only
//! allows a CNAME *or* other record types at a given name, never both. When a
//! write fails with a `DUPLICATE_RECORD` validation issue, this looks up the
//! records actually at that name and explains precisely what's blocking it.

use crate::domain::{api_error, format_api_error};

use domains_client::types;

use super::records::fetch_records;

/// A v3 validation-error body's `details[].issue` codes. Deliberately not
/// `domain::common`'s `ApiErrorBody` — that type is about rendering a friendly
/// message, this is about branching on the specific code GoDaddy uses for a
/// DNS name-exclusivity conflict.
#[derive(serde::Deserialize)]
struct DuplicateRecordBody {
    #[serde(default)]
    details: Vec<DuplicateRecordDetail>,
}

#[derive(serde::Deserialize)]
struct DuplicateRecordDetail {
    #[serde(default)]
    issue: String,
}

/// Whether a v3 error response body reports a `DUPLICATE_RECORD` validation
/// issue — GoDaddy's code for a DNS name-exclusivity conflict (e.g. a CNAME
/// already at a name a caller is trying to add an A record to). `false` for
/// any other issue code, an unparseable body, or a body with no `details`.
pub(super) fn duplicate_record_issue(body: &str) -> bool {
    serde_json::from_str::<DuplicateRecordBody>(body)
        .ok()
        .is_some_and(|b| b.details.iter().any(|d| d.issue == "DUPLICATE_RECORD"))
}

/// The existing records at a name that would conflict with writing
/// `desired_type` there, per DNS's CNAME-exclusivity rule (RFC 1034/1035): a
/// name can have a CNAME *or* other record types, never both. If `desired_type`
/// is CNAME, every non-CNAME record at the name conflicts; otherwise, any
/// existing CNAME does.
pub(super) fn conflicting_records<'a>(
    desired_type: &str,
    existing_at_name: &'a [types::DnsRecord],
) -> Vec<&'a types::DnsRecord> {
    if desired_type == "CNAME" {
        existing_at_name
            .iter()
            .filter(|r| r.type_.as_str() != "CNAME")
            .collect()
    } else {
        existing_at_name
            .iter()
            .filter(|r| r.type_.as_str() == "CNAME")
            .collect()
    }
}

/// Explain a `DUPLICATE_RECORD` failure in DNS terms, given the records actually
/// at the name. Three cases: a real CNAME-exclusivity conflict (names what's
/// there, and how to resolve it — automatically for `set` if `can_auto_replace`,
/// else a manual pointer); an exact same-type-same-data match; or — anything
/// else `DUPLICATE_RECORD` could mean (e.g. a second, different-valued CNAME) —
/// a generic pointer at `dns list`.
pub(super) fn describe_duplicate_record(
    desired_type: &str,
    desired_data: &str,
    domain: &str,
    name: &str,
    at_name: &[types::DnsRecord],
    can_auto_replace: bool,
) -> String {
    let conflicts = conflicting_records(desired_type, at_name);
    if !conflicts.is_empty() {
        let mut conflict_types: Vec<&str> = conflicts.iter().map(|r| r.type_.as_str()).collect();
        conflict_types.sort_unstable();
        conflict_types.dedup();

        let remediation = if can_auto_replace {
            "\n\nRe-run with `--replace-conflicting-types` to remove the conflicting record(s) \
             automatically."
                .to_string()
        } else {
            // A CNAME conflict can span multiple non-CNAME types (e.g. A + MX) at
            // the same name — every one of them has to be deleted before the add
            // can succeed, not just the first.
            let deletes = conflict_types
                .iter()
                .map(|t| format!("gddy dns delete {domain} --type {t} --name {name}"))
                .collect::<Vec<_>>()
                .join("; ");
            format!(
                "\n\nRun `{deletes}` first, then re-run `gddy dns add {domain} --type \
                 {desired_type} --name {name} --data {desired_data}`.",
            )
        };
        return if desired_type == "CNAME" {
            format!(
                "`{name}` already has other record(s) ({}) at this name, which can't coexist \
                 with a CNAME — DNS only allows one or the other at a given name.{remediation}",
                conflict_types.join(", "),
            )
        } else {
            format!(
                "`{name}` already has a CNAME record (→ `{}`), which can't coexist with \
                 {desired_type} records — DNS only allows one or the other at a given \
                 name.{remediation}",
                conflicts[0].data,
            )
        };
    }

    if at_name
        .iter()
        .any(|r| r.type_.as_str() == desired_type && r.data == desired_data)
    {
        return format!("a {desired_type} record with this exact value already exists at {name}.");
    }

    format!(
        "`{name}` already has conflicting {desired_type} data — run `gddy dns list {domain} \
         --type {desired_type} --name {name}` to see the current records before retrying."
    )
}

/// Fetch every record at a name (all types), for diagnosing a name-exclusivity
/// conflict after a write already failed. Reuses [`fetch_records`] with no
/// type filter — unlike `dns list`, which requires `--type` whenever `--name`
/// is given, this deliberately fetches every type at the name, since the
/// conflict could be with any of them.
pub(super) async fn conflicting_records_at(
    client: &domains_client::Client,
    domain: &str,
    name: &str,
    debug: bool,
) -> Result<Vec<types::DnsRecord>, cli_engine::CliCoreError> {
    fetch_records(client, domain, None, Some(name), debug).await
}

/// Context for a failed `add` write, bundled to keep [`describe_write_error`]'s
/// signature within clippy's argument-count limit.
pub(super) struct WriteErrorContext<'a> {
    pub(super) domain: &'a str,
    pub(super) name: &'a str,
    pub(super) desired_type: &'a str,
    pub(super) desired_data: &'a str,
    pub(super) action: &'a str,
    pub(super) debug: bool,
}

/// Turn a failed `add` write into a user-facing error message. If the response
/// is a name-exclusivity conflict (`DUPLICATE_RECORD`), look up the records at
/// this name and explain precisely what's blocking it; otherwise fall back to
/// the generic `domain::common` formatting (still improved by its v3
/// `details[]` support). `add` never auto-replaces, so the remediation is
/// always the manual pointer (`can_auto_replace: false`).
pub(super) async fn describe_write_error(
    client: &domains_client::Client,
    ctx: &WriteErrorContext<'_>,
    err: domains_client::Error<()>,
) -> String {
    let domains_client::Error::UnexpectedResponse(resp) = err else {
        return api_error(ctx.action, ctx.debug, err).await.to_string();
    };
    let status = resp.status();
    let request_id = resp
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let body = resp.text().await.unwrap_or_default();
    if !duplicate_record_issue(&body) {
        return format_api_error(
            ctx.action,
            status.as_u16(),
            &status.to_string(),
            &body,
            request_id.as_deref(),
            ctx.debug,
        );
    }
    match conflicting_records_at(client, ctx.domain, ctx.name, ctx.debug).await {
        Ok(at_name) => describe_duplicate_record(
            ctx.desired_type,
            ctx.desired_data,
            ctx.domain,
            ctx.name,
            &at_name,
            false,
        ),
        Err(_) => format_api_error(
            ctx.action,
            status.as_u16(),
            &status.to_string(),
            &body,
            request_id.as_deref(),
            ctx.debug,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use super::super::records::DEFAULT_TTL;

    fn dns_record(ty: &str, data: &str) -> types::DnsRecord {
        types::DnsRecord {
            data: data.to_string(),
            flag: None,
            name: "www".to_string(),
            port: None,
            priority: None,
            protocol: None,
            record_id: Some(format!("{ty}-id")),
            service: None,
            tag: None,
            ttl: DEFAULT_TTL,
            type_: types::DnsRecordType(ty.to_string()),
            weight: None,
        }
    }

    #[test]
    fn duplicate_record_issue_detects_the_v3_issue_code() {
        // The exact body the reporting user got back from a `dns set` CNAME conflict.
        let body = r#"{"correlationId":"abc-123","details":[{"description":"Duplicate data provided for record name, www.","issue":"DUPLICATE_RECORD"}],"message":"Request failed validation","name":"VALIDATION_ERROR"}"#;
        assert!(duplicate_record_issue(body));

        let unrelated = r#"{"name":"VALIDATION_ERROR","message":"bad","details":[{"issue":"INVALID_FORMAT","description":"nope"}]}"#;
        assert!(!duplicate_record_issue(unrelated));

        assert!(!duplicate_record_issue("not json"));
        assert!(!duplicate_record_issue(""));
    }

    #[test]
    fn conflicting_records_enforces_cname_exclusivity() {
        let cname_only = vec![dns_record("CNAME", "parkpage.godaddy.com")];
        assert_eq!(conflicting_records("A", &cname_only).len(), 1);

        let others = vec![
            dns_record("A", "1.2.3.4"),
            dns_record("MX", "mail.example.com"),
        ];
        assert_eq!(conflicting_records("CNAME", &others).len(), 2);

        // Same type as desired → not a conflict.
        let same_type = vec![dns_record("A", "1.2.3.4")];
        assert!(conflicting_records("A", &same_type).is_empty());

        assert!(conflicting_records("A", &[]).is_empty());
    }

    #[test]
    fn describe_duplicate_record_explains_cname_exclusivity() {
        let at_name = vec![dns_record("CNAME", "parkpage.godaddy.com")];

        let msg = describe_duplicate_record("A", "1.2.3.4", "example.com", "www", &at_name, true);
        assert!(msg.contains("already has a CNAME record"), "{msg}");
        assert!(msg.contains("parkpage.godaddy.com"), "{msg}");
        assert!(msg.contains("--replace-conflicting-types"), "{msg}");

        let msg_no_flag =
            describe_duplicate_record("A", "1.2.3.4", "example.com", "www", &at_name, false);
        assert!(
            msg_no_flag.contains("dns delete example.com --type CNAME --name www"),
            "{msg_no_flag}"
        );

        // The reverse conflict: setting a CNAME where other types already exist.
        let others = vec![
            dns_record("A", "1.2.3.4"),
            dns_record("MX", "mail.example.com"),
        ];
        let msg = describe_duplicate_record(
            "CNAME",
            "target.example.net",
            "example.com",
            "www",
            &others,
            true,
        );
        assert!(msg.contains("can't coexist with a CNAME"), "{msg}");
        assert!(msg.contains("A, MX"), "{msg}");

        // Manual remediation for a multi-type conflict must name every
        // conflicting type's delete command, not just the first.
        let msg_no_flag = describe_duplicate_record(
            "CNAME",
            "target.example.net",
            "example.com",
            "www",
            &others,
            false,
        );
        assert!(
            msg_no_flag.contains("dns delete example.com --type A --name www"),
            "{msg_no_flag}"
        );
        assert!(
            msg_no_flag.contains("dns delete example.com --type MX --name www"),
            "{msg_no_flag}"
        );
    }

    #[test]
    fn describe_duplicate_record_names_exact_and_generic_cases() {
        // Exact same-type-same-data match.
        let exact = vec![dns_record("A", "1.2.3.4")];
        let msg = describe_duplicate_record("A", "1.2.3.4", "example.com", "www", &exact, true);
        assert!(msg.contains("exact value already exists"), "{msg}");

        // Same type, different data — DUPLICATE_RECORD but not a cross-type
        // conflict → generic fallback pointing at `dns list`.
        let different_data = vec![dns_record("A", "9.9.9.9")];
        let msg =
            describe_duplicate_record("A", "1.2.3.4", "example.com", "www", &different_data, true);
        assert!(msg.contains("already has conflicting A data"), "{msg}");
        assert!(
            msg.contains("dns list example.com --type A --name www"),
            "{msg}"
        );
    }
}
