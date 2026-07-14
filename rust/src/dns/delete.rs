//! `dns delete` — remove all records matching a type+name from a domain.

use cli_engine::{CliCoreError, CommandResult, CommandSpec, RuntimeCommandSpec, Tier};
use serde_json::{Value, json};

use crate::domain::{api_error, make_client};
use crate::output_schema::output_schema;
use crate::scopes::DOMAINS_DNS_UPDATE;

use super::records::{arg_str, fetch_records, parse_write_type_arg, verify_with_list_action};

output_schema!(DnsDeleteResult {
    "domain": "string";
    "type": "string";
    "name": "string";
    "deleted": "number";
    "failed": "number";
    "action": "string";
});

/// Build the `delete` result from per-record delete outcomes (each matched
/// record's `data` value paired with `None` on success or `Some(error)`):
/// `Ok(json)` with the deleted count (0 when nothing matched) if all succeeded,
/// else `Err(message)` — a non-zero exit — with a per-record ✓/✗ breakdown. Pure
/// so it's unit-testable.
fn summarize_delete_outcomes(
    domain: &str,
    record_type: &str,
    name: &str,
    outcomes: &[(String, Option<String>)],
) -> Result<Value, String> {
    let failed = outcomes.iter().filter(|(_, e)| e.is_some()).count();
    let deleted = outcomes.len() - failed;

    if failed > 0 {
        let breakdown = outcomes
            .iter()
            .map(|(value, err)| match err {
                None => format!("  ✓ {value} — deleted"),
                Some(e) => format!("  ✗ {value} — {e}"),
            })
            .collect::<Vec<_>>()
            .join("\n");
        return Err(format!(
            "deleted {deleted} of {} record(s) for {name} ({record_type}); {failed} failed:\n\
             {breakdown}\n\nRe-run `gddy dns delete {domain} --type {record_type} --name \
             {name}`, or `gddy dns list {domain} --type {record_type} --name {name}` to \
             review the current state.",
            outcomes.len(),
        ));
    }

    Ok(json!({
        "domain": domain,
        "type": record_type,
        "name": name,
        "deleted": deleted,
        "failed": failed,
        "action": "delete",
    }))
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_with_context(
        CommandSpec::new(
            "delete",
            "Delete all records for a type+name (destructive: removes existing)",
        )
        .with_long(
            "Removes every DNS record matching the given type+name pair. v3 deletes \
             one record at a time, so this lists the matching records and deletes \
             each; a partial failure is reported per record. This is destructive \
             and irreversible. NS and SOA records are GoDaddy-managed and cannot be \
             deleted. Use `dns list` to confirm what will be removed first.",
        )
        .with_system("domain")
        .with_tier(Tier::Destructive)
        .with_default_fields("domain,type,name,deleted,failed")
        .with_output_schema::<DnsDeleteResult>()
        .with_scopes(&[DOMAINS_DNS_UPDATE])
        .with_arg(
            clap::Arg::new("domain")
                .value_name("DOMAIN")
                .required(true)
                .help("Domain whose records to delete (e.g. example.com)"),
        )
        .with_arg(
            clap::Arg::new("type")
                .long("type")
                .value_name("TYPE")
                .required(true)
                .value_parser(parse_write_type_arg)
                .help("Record type (A, AAAA, ALIAS, CAA, CNAME, MX, SRV, TXT)"),
        )
        .with_arg(
            clap::Arg::new("name")
                .long("name")
                .value_name("NAME")
                .required(true)
                .help("Record name relative to the domain (e.g. www)"),
        ),
        |ctx| async move {
            let domain = arg_str(&ctx, "domain").unwrap_or_default();
            let record_type = arg_str(&ctx, "type").unwrap_or_default();
            let name = arg_str(&ctx, "name").unwrap_or_default();

            let debug = !ctx.middleware.debug.is_empty();
            let client = make_client(&ctx).await?;

            // v3 deletes by record id, so find the matching records first.
            let existing = fetch_records(
                &client,
                domain.as_str(),
                Some(&record_type),
                Some(&name),
                debug,
            )
            .await?;

            let mut outcomes = Vec::with_capacity(existing.len());
            for rec in &existing {
                // A record with no server id can't be targeted — report it as a
                // failure (non-zero exit) rather than silently leaving it behind.
                let err = match rec.record_id.as_deref() {
                    None => Some(format!(
                        "the API returned this record without a recordId, so it can't be \
                         deleted; re-run `gddy dns list {domain} --type {record_type} --name \
                         {name}` and remove it in the control panel if it persists"
                    )),
                    Some(id) => match client
                        .delete_dns_record()
                        .zone(domain.as_str())
                        .record_id(id)
                        .send()
                        .await
                    {
                        Ok(_) => None,
                        Err(e) => {
                            Some(api_error("deleting DNS record", debug, e).await.to_string())
                        }
                    },
                };
                outcomes.push((rec.data.clone(), err));
            }

            summarize_delete_outcomes(&domain, &record_type, &name, &outcomes)
                .map(|v| {
                    CommandResult::new(v).with_next_actions(vec![verify_with_list_action(
                        &domain,
                        &record_type,
                        &name,
                    )])
                })
                .map_err(CliCoreError::message)
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summarize_delete_reports_count_zero_and_partial_failure() {
        // Nothing matched → deleted 0, not an error.
        let none: [(String, Option<String>); 0] = [];
        let v = summarize_delete_outcomes("example.com", "A", "www", &none).expect("ok");
        assert_eq!(v["deleted"], 0);
        // All deleted.
        let ok = vec![("1.2.3.4".to_string(), None), ("5.6.7.8".to_string(), None)];
        let v = summarize_delete_outcomes("example.com", "A", "www", &ok).expect("ok");
        assert_eq!(v["deleted"], 2);
        // One failed → non-zero error naming the failed value.
        let mixed = vec![
            ("1.2.3.4".to_string(), None),
            ("5.6.7.8".to_string(), Some("nope".to_string())),
        ];
        let err =
            summarize_delete_outcomes("example.com", "A", "www", &mixed).expect_err("a failure");
        assert!(err.contains("deleted 1 of 2"), "{err}");
        assert!(err.contains("✗ 5.6.7.8 — nope"), "{err}");
        // The recovery hint must include the domain positional and flags or it won't parse.
        assert!(
            err.contains("dns delete example.com --type A --name www"),
            "{err}"
        );
        assert!(
            err.contains("dns list example.com --type A --name www"),
            "{err}"
        );
    }
}
