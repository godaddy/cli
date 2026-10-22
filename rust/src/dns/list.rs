//! `dns list` — list DNS records for a domain, with optional type/name filters.

use cli_engine::{
    Alignment, CliCoreError, CommandResult, CommandSpec, CursorConfig, CursorContinuation,
    RuntimeCommandSpec, TableColumn, Tier,
};
use serde_json::{Value, json};

use crate::domain::make_client;
use crate::pagination_token;
use crate::scopes::DOMAINS_READ;

use domains_client::types;

use super::records::{MAX_PAGE_SIZE, fetch_records_page, merged_tlsa_data, parse_list_type_arg};

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

    /// Jump directly to a page of results (1-based). Ignored when `--continue` is given.
    #[arg(long, value_name = "N", value_parser = clap::value_parser!(u64).range(1..))]
    page: Option<u64>,
}

/// A record is unreadable without its type/name/data, so those three are
/// `essential` — never hidden or shrunk for width, even on a narrow terminal
/// — while the rest are nice-to-haves that still yield to width pressure.
fn view_columns() -> Vec<TableColumn> {
    vec![
        TableColumn::new("type", "Type").essential(true),
        TableColumn::new("name", "Name").essential(true),
        TableColumn::new("data", "Data").essential(true),
        TableColumn::new("ttl", "TTL").align(Alignment::Right),
        TableColumn::new("priority", "Priority").align(Alignment::Right),
        TableColumn::new("recordId", "Record ID"),
        TableColumn::new("service", "Service"),
        TableColumn::new("protocol", "Protocol"),
        TableColumn::new("port", "Port").align(Alignment::Right),
        TableColumn::new("weight", "Weight").align(Alignment::Right),
        TableColumn::new("tag", "Tag"),
        TableColumn::new("flag", "Flag").align(Alignment::Right),
        TableColumn::new("usage", "Usage"),
        TableColumn::new("selector", "Selector"),
        TableColumn::new("matchingType", "Matching Type"),
        TableColumn::new("certificateData", "Certificate Data").no_truncate(true),
        TableColumn::new("parameters", "Parameters"),
    ]
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
            .with_view(view_columns())
            .with_scopes(&[DOMAINS_READ])
            .with_cursor(CursorConfig::new(50, MAX_PAGE_SIZE.get() as i64)),
        |ctx, args: ListArgs| async move {
            let domain = args.domain;
            let type_opt = args.record_type;
            let name_opt = args.name;

            let debug = !ctx.middleware.debug.is_empty();
            let client = make_client(&ctx).await?;
            let (page, page_size) = pagination_token::resolve_page_request(
                args.page,
                ctx.middleware.continue_token.as_deref(),
                1u64,
                ctx.middleware.cursor_limit,
                MAX_PAGE_SIZE.get() as i64,
            );
            let fetched = fetch_records_page(
                &client,
                domain.as_str(),
                type_opt.as_deref(),
                name_opt.as_deref(),
                page,
                page_size,
                debug,
            )
            .await?;

            let out = build_list_output(&fetched.records).map_err(|e| {
                CliCoreError::message(format!("failed to serialize DNS records: {e}"))
            })?;
            let mut continuation = if fetched.has_more {
                CursorContinuation::more(pagination_token::build(&[
                    ("limit", &page_size.to_string()),
                    ("page", &(page + 1).to_string()),
                ]))
            } else {
                CursorContinuation::done()
            };
            if let Some(total) = fetched.total {
                continuation = continuation
                    .with_total(total)
                    .with_remaining(remaining_after(
                        total,
                        page,
                        page_size,
                        fetched.records.len(),
                    ));
            }
            let continuation = continuation.with_limit(page_size);
            Ok(CommandResult::new(json!(out)).with_cursor(continuation))
        },
    )
}

/// Records left after `page` (1-based) of `page_size`, given the API's `total`
/// and how many records that page returned. Saturating throughout, so a
/// hand-edited `--continue` token or an out-of-range `--page` can't underflow
/// or go negative.
fn remaining_after(total: i64, page: u64, page_size: i64, returned: usize) -> i64 {
    let page_size = page_size.clamp(1, MAX_PAGE_SIZE.get() as i64);
    let before = i64::try_from(page.saturating_sub(1))
        .unwrap_or(i64::MAX)
        .saturating_mul(page_size);
    let seen = before.saturating_add(i64::try_from(returned).unwrap_or(i64::MAX));
    total.saturating_sub(seen).max(0)
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
    use cli_engine::CursorConfig;

    #[test]
    fn remaining_after_counts_what_is_left_past_the_current_page() {
        assert_eq!(remaining_after(250, 1, 100, 100), 150);
        assert_eq!(remaining_after(250, 2, 100, 100), 50);
        assert_eq!(remaining_after(250, 3, 100, 50), 0);
    }

    #[test]
    fn remaining_after_never_goes_negative_or_overflows() {
        // Page past the end, an oversized page size (clamped), and extreme values.
        assert_eq!(remaining_after(10, 9, 100, 0), 0);
        assert_eq!(remaining_after(250, 1, 10_000, 100), 150);
        assert_eq!(remaining_after(i64::MAX, u64::MAX, i64::MAX, usize::MAX), 0);
        assert_eq!(remaining_after(0, 1, 50, 0), 0);
    }

    #[test]
    fn opts_into_cursor_pagination_with_a_default_and_a_max_limit() {
        assert_eq!(
            command().spec.cursor,
            Some(CursorConfig::new(50, MAX_PAGE_SIZE.get() as i64))
        );
    }

    /// `view_columns()` declares 17 columns — far more than fit an
    /// 80-column terminal (the fixed width non-TTY test runs get) once
    /// their headers are laid out side by side. Type/name/data must survive
    /// that squeeze regardless, since a record is unreadable without them;
    /// everything else is free to be hidden.
    #[test]
    fn essential_fields_survive_width_based_hiding() {
        let out = build_list_output(&[a_record()]).expect("serializes");

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
