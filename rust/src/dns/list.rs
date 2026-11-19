//! `dns list` — list DNS records for a domain, with optional type/name filters.

use cli_engine::{
    Alignment, CliCoreError, CommandResult, CommandSpec, PaginationConfig, RuntimeCommandSpec,
    TableColumn, Tier,
};
use serde_json::{Value, json};

use crate::domain::make_client;
use crate::idn::DomainFormArgs;
use crate::scopes::DOMAINS_READ;

use domains_client::types;

use super::records::{detail_columns, fetch_records, merged_tlsa_data, parse_list_type_arg};

#[derive(Debug, Clone, clap::Args)]
struct ListArgs {
    /// Domain whose records to list (e.g. example.com).
    #[arg(value_name = "DOMAIN", value_parser = crate::idn::parse_name_arg)]
    domain: String,

    /// Only records of this type (A, AAAA, ALIAS, CAA, CNAME, HTTPS, MX, NS,
    /// SOA, SRV, SVCB, TLSA, TXT).
    #[arg(long = "type", value_name = "TYPE", value_parser = parse_list_type_arg)]
    record_type: Option<String>,

    /// Only records with this name (requires --type).
    #[arg(long, value_name = "NAME", requires = "record_type", value_parser = crate::idn::parse_name_arg)]
    name: Option<String>,

    #[command(flatten)]
    domain_format: DomainFormArgs,
}

/// The columns `dns list` shows when `--fields` is not given, in display order.
/// Everything after `ttl` is specific to some record types and only appears when
/// a listed record has it (see `view_columns`). The TLSA fields are left out
/// because `data` already carries them as `usage selector matching-type cert`.
const DEFAULT_FIELDS: &str =
    "type,name,data,ttl,priority,weight,port,service,protocol,flag,tag,parameters";

/// A record is unreadable without its type/name/data, so those three are
/// `essential` — never hidden or shrunk for width, even on a narrow terminal
/// — while the rest are nice-to-haves that still yield to width pressure.
fn view_columns() -> Vec<TableColumn> {
    // The record-specific columns are only worth a column when some record uses
    // them, so a listing of A and TXT records looks as it always did while SRV,
    // MX, CAA and TLSA records bring the fields that tell them apart.
    let sparse = |column: TableColumn| column.hide_when_empty(true);
    let mut columns = vec![
        TableColumn::new("type", "Type").essential(true),
        TableColumn::new("name", "Name").essential(true),
        TableColumn::new("data", "Data").essential(true),
        TableColumn::new("ttl", "TTL").align(Alignment::Right),
    ];
    columns.extend(detail_columns());
    columns.extend([
        TableColumn::new("recordId", "Record ID"),
        sparse(TableColumn::new("usage", "Usage")),
        sparse(TableColumn::new("selector", "Selector")),
        sparse(TableColumn::new("matchingType", "Matching Type")),
        sparse(TableColumn::new("certificateData", "Certificate Data").no_truncate(true)),
    ]);
    columns
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
            .with_default_fields(DEFAULT_FIELDS)
            .with_json_schema::<types::DnsRecord>()
            .with_view(view_columns())
            .with_scopes(&[DOMAINS_READ])
            .with_pagination(PaginationConfig::new(0, 500)),
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

            let mut out = build_list_output(&records).map_err(|e| {
                CliCoreError::message(format!("failed to serialize DNS records: {e}"))
            })?;
            let domain_format = args.domain_format.form(&ctx)?;
            for record in &mut out {
                domain_format.convert_fields(record, &["name"]);
            }
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
pub(super) fn build_list_output(
    records: &[types::DnsRecord],
) -> Result<Vec<Value>, serde_json::Error> {
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
            Some(PaginationConfig::new(0, 500))
        );
    }

    fn srv_record(id: &str, service: &str, port: u16) -> types::DnsRecord {
        types::DnsRecord {
            data: Some("chat-server.example.com.".to_string()),
            name: "chat".to_string(),
            port: Some(port),
            priority: Some(10),
            weight: Some(5),
            protocol: Some("_tcp".to_string()),
            service: Some(service.to_string()),
            record_id: Some(id.to_string()),
            type_: types::DnsRecordType("SRV".to_string()),
            ..a_record()
        }
    }

    /// The default fields as the engine applies them: the view's columns
    /// narrowed to `DEFAULT_FIELDS`, in that order.
    fn default_columns() -> Vec<TableColumn> {
        let all = view_columns();
        DEFAULT_FIELDS
            .split(',')
            .filter_map(|field| all.iter().find(|c| c.field == field).cloned())
            .collect()
    }

    /// `view_columns()` declares 17 columns — far more than fit an
    /// 80-column terminal (the fixed width non-TTY test runs get) once
    /// their headers are laid out side by side. Type/name/data must survive
    /// that squeeze regardless, since a record is unreadable without them;
    /// everything else is free to be hidden.
    #[test]
    fn essential_fields_survive_width_based_hiding() {
        // Records that use every optional column, so the table is wider than
        // 80 characters and the engine has to decide what to hide.
        let out = build_list_output(&[srv_record("r1", "_xmpp", 5222)]).expect("serializes");

        let rendered = cli_engine::preview_human_view(out, &view_columns());

        let header_line = rendered.lines().next().expect("header line");
        assert!(header_line.contains("TYPE"), "{rendered}");
        assert!(header_line.contains("NAME"), "{rendered}");
        assert!(header_line.contains("DATA"), "{rendered}");
        assert!(
            rendered.contains("hidden to fit the display width"),
            "fixture must actually overflow 80 columns to exercise hiding: {rendered}"
        );
    }

    #[test]
    fn a_listing_without_srv_or_tlsa_records_has_no_extra_columns() {
        let out = build_list_output(&[a_record()]).expect("serializes");

        let rendered = cli_engine::preview_human_view(out, &default_columns());

        let header_line = rendered.lines().next().expect("header line");
        for shown in ["TYPE", "NAME", "DATA", "TTL"] {
            assert!(header_line.contains(shown), "{rendered}");
        }
        for sparse in [
            "PORT", "SERVICE", "PROTOCOL", "PRIORITY", "WEIGHT", "TAG", "FLAG",
        ] {
            assert!(!header_line.contains(sparse), "{sparse} shown: {rendered}");
        }
    }

    #[test]
    fn srv_records_under_one_name_are_told_apart_by_service_and_port() {
        let out = build_list_output(&[
            srv_record("r1", "_sip", 5060),
            srv_record("r2", "_xmpp", 5222),
        ])
        .expect("serializes");

        let rendered = cli_engine::preview_human_view(out, &default_columns());

        // At 80 columns the trailing protocol column is the one hidden for
        // width; the rest are what tells the two rows apart.
        for shown in ["PORT", "SERVICE", "PRIORITY", "WEIGHT"] {
            assert!(rendered.contains(shown), "{shown} missing: {rendered}");
        }
        assert!(
            rendered.contains("_sip") && rendered.contains("5060"),
            "{rendered}"
        );
        assert!(
            rendered.contains("_xmpp") && rendered.contains("5222"),
            "{rendered}"
        );
    }

    #[test]
    fn a_mixed_listing_shows_the_extra_columns_with_blanks_on_other_rows() {
        let out =
            build_list_output(&[a_record(), srv_record("r2", "_xmpp", 5222)]).expect("serializes");

        let rendered = cli_engine::preview_human_view(out, &default_columns());

        assert!(
            rendered.contains("SERVICE") && rendered.contains("_xmpp"),
            "{rendered}"
        );
        let a_row = rendered
            .lines()
            .find(|l| l.contains("1.2.3.4"))
            .expect("A row");
        assert!(
            !a_row.contains("_xmpp") && !a_row.contains("5222"),
            "{a_row}"
        );
    }

    #[test]
    fn the_default_fields_are_the_core_columns_then_the_shared_detail_fields() {
        let expected = format!(
            "type,name,data,ttl,{}",
            super::super::records::DETAIL_FIELDS.join(",")
        );
        assert_eq!(DEFAULT_FIELDS, expected);
    }

    #[test]
    fn every_default_field_is_a_declared_column() {
        let declared: Vec<String> = view_columns().into_iter().map(|c| c.field).collect();
        for field in DEFAULT_FIELDS.split(',') {
            assert!(declared.iter().any(|d| d == field), "{field} has no column");
        }
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
