//! `dns list` — list DNS records for a domain, with optional type/name filters.

use cli_engine::{
    CliCoreError, CommandResult, CommandSpec, PaginationConfig, RuntimeCommandSpec, Tier,
};
use serde_json::{Value, json};

use crate::domain::make_client;
use crate::scopes::DOMAINS_READ;

use domains_client::types;

use super::records::{fetch_records, merged_tlsa_data, parse_list_type_arg};

#[derive(Debug, Clone, clap::Args)]
struct ListArgs {
    /// Domain whose records to list (e.g. example.com).
    #[arg(value_name = "DOMAIN")]
    domain: String,

    /// Only records of this type (A, AAAA, ALIAS, CAA, CNAME, HTTPS, MX, NS,
    /// SOA, SRV, SVCB, TLSA, TXT).
    #[arg(long = "type", value_name = "TYPE", value_parser = parse_list_type_arg)]
    record_type: Option<String>,

    /// Only records with this name (requires --type).
    #[arg(long, value_name = "NAME", requires = "record_type")]
    name: Option<String>,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<ListArgs, _, _, _>(
        CommandSpec::from_args::<ListArgs>("list", "List DNS records for a domain")
            .with_long(
                "Retrieves DNS records for a domain. Without filters, returns all \
                 record types. Use `--type` to narrow to one record type, and \
                 `--name` (requires `--type`) to further narrow to a specific name.",
            )
            .with_system("domain")
            .with_tier(Tier::Read)
            .with_default_fields("type,name,data,ttl")
            .with_json_schema::<types::DnsRecord>()
            .with_scopes(&[DOMAINS_READ])
            .with_pagination(PaginationConfig {
                max_limit: 500,
                ..Default::default()
            }),
        |ctx, args: ListArgs| async move {
            let domain = args.domain;
            let type_opt = args.record_type;
            let name_opt = args.name;

            let debug = !ctx.middleware.debug.is_empty();
            let client = make_client(&ctx).await?;
            let records = fetch_records(
                &client,
                domain.as_str(),
                type_opt.as_deref(),
                name_opt.as_deref(),
                debug,
            )
            .await?;

            let out = build_list_output(&records).map_err(|e| {
                CliCoreError::message(format!("failed to serialize DNS records: {e}"))
            })?;
            Ok(CommandResult::new(json!(out)))
        },
    )
}

/// Serialize fetched records for `dns list`'s output, re-merging TLSA's
/// split fields back into a single `data` string (see
/// [`merged_tlsa_data`]) — every other type already has a meaningful `data`.
/// The original `certificateData`/`usage`/`selector`/`matchingType` fields
/// are left in place alongside it. Pure so the merge is unit-testable
/// without a mock server.
fn build_list_output(records: &[types::DnsRecord]) -> Result<Vec<Value>, serde_json::Error> {
    records
        .iter()
        .map(|rec| {
            let mut v = serde_json::to_value(rec)?;
            if rec.type_.as_str() == "TLSA"
                && let Some(obj) = v.as_object_mut()
            {
                obj.insert("data".to_string(), json!(merged_tlsa_data(rec)));
            }
            Ok(v)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use cli_engine::PaginationConfig;

    #[test]
    fn opts_into_pagination_with_no_default_and_a_max_limit() {
        assert_eq!(
            command().spec.pagination,
            Some(PaginationConfig {
                max_limit: 500,
                ..Default::default()
            })
        );
    }

    fn a_record() -> types::DnsRecord {
        types::DnsRecord {
            certificate_data: None,
            matching_type: None,
            selector: None,
            usage: None,
            data: Some("1.2.3.4".to_string()),
            flag: None,
            name: "www".to_string(),
            parameters: None,
            port: None,
            priority: None,
            protocol: None,
            record_id: Some("r1".to_string()),
            service: None,
            tag: None,
            ttl: 3600,
            type_: types::DnsRecordType("A".to_string()),
            weight: None,
        }
    }

    fn tlsa_record(cert: &str) -> types::DnsRecord {
        types::DnsRecord {
            certificate_data: Some(cert.to_string()),
            matching_type: Some(types::TlsaMatchingType(1)),
            selector: Some(types::TlsaSelector(1)),
            usage: Some(types::TlsaUsage(3)),
            data: None,
            type_: types::DnsRecordType("TLSA".to_string()),
            ..a_record()
        }
    }

    /// DNS treats a TLSA record's RDATA as one blob (RFC 6698) — `dns list`
    /// re-merges the split fields into `data` for display, without dropping
    /// the originals.
    #[test]
    fn build_list_output_merges_tlsa_fields_into_data() {
        let cert = "d2abde240d7cd3ee6b4b28c54df034b97983a1d16e8a410e4561cb106618e971";
        let out = build_list_output(&[tlsa_record(cert)]).expect("serializes");
        assert_eq!(out[0]["data"], format!("3 1 1 {cert}"));
        assert_eq!(out[0]["certificateData"], cert);
        assert_eq!(out[0]["usage"], 3);
        assert_eq!(out[0]["selector"], 1);
        assert_eq!(out[0]["matchingType"], 1);
    }

    #[test]
    fn build_list_output_leaves_other_types_data_untouched() {
        let out = build_list_output(&[a_record()]).expect("serializes");
        assert_eq!(out[0]["data"], "1.2.3.4");
    }
}
