//! `dns list` — list DNS records for a domain, with optional type/name filters.

use cli_engine::{CliCoreError, CommandResult, CommandSpec, RuntimeCommandSpec, Tier};
use serde_json::{Value, json};

use crate::domain::make_client;
use crate::scopes::DOMAINS_READ;

use domains_client::types;

use super::records::{arg_str, fetch_records, parse_list_type_arg};

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_with_context(
        CommandSpec::new("list", "List DNS records for a domain")
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
            .with_arg(
                clap::Arg::new("domain")
                    .value_name("DOMAIN")
                    .required(true)
                    .help("Domain whose records to list (e.g. example.com)"),
            )
            .with_arg(
                clap::Arg::new("type")
                    .long("type")
                    .value_name("TYPE")
                    .value_parser(parse_list_type_arg)
                    .help(
                        "Only records of this type (A, AAAA, ALIAS, CAA, CNAME, MX, NS, SOA, \
                         SRV, TXT)",
                    ),
            )
            .with_arg(
                clap::Arg::new("name")
                    .long("name")
                    .value_name("NAME")
                    .requires("type")
                    .help("Only records with this name (requires --type)"),
            ),
        |ctx| async move {
            let domain = arg_str(&ctx, "domain").unwrap_or_default();
            let type_opt = arg_str(&ctx, "type");
            let name_opt = arg_str(&ctx, "name");

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
