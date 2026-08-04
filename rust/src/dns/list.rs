//! `dns list` — list DNS records for a domain, with optional type/name filters.

use cli_engine::{CliCoreError, CommandResult, CommandSpec, RuntimeCommandSpec, Tier};
use serde_json::{Value, json};

use crate::domain::make_client;
use crate::scopes::DOMAINS_READ;

use domains_client::types;

use super::records::{fetch_records, parse_list_type_arg};

#[derive(Debug, Clone, clap::Args)]
struct ListArgs {
    /// Domain whose records to list (e.g. example.com).
    #[arg(value_name = "DOMAIN")]
    domain: String,

    /// Only records of this type (A, AAAA, ALIAS, CAA, CNAME, MX, NS, SOA,
    /// SRV, TXT).
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
            .with_scopes(&[DOMAINS_READ]),
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

            let out: Vec<Value> = records
                .iter()
                .map(serde_json::to_value)
                .collect::<Result<_, _>>()
                .map_err(|e| {
                    CliCoreError::message(format!("failed to serialize DNS records: {e}"))
                })?;
            Ok(CommandResult::new(json!(out)))
        },
    )
}
