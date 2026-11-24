//! `dns export` — write a domain's DNS records as a zone file (or as JSON) that
//! `dns import` reads back.

use std::io::Write;

use cli_engine::{CliCoreError, CommandResult, CommandSpec, RuntimeCommandSpec, Tier};
use domains_client::types;
use serde_json::json;

use crate::domain::make_client;
use crate::scopes::DOMAINS_READ;

use super::list::build_list_output;
use super::records::fetch_records;
use super::zone_records::{
    describe_record, export_zone, is_nonstandard, is_provider_managed, split_nonstandard,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum ExportFormat {
    /// A zone file in the RFC 1035 master file format (also called BIND format),
    /// which other DNS providers and tools read too.
    Zone,
    /// The records as JSON, which `dns import` reads and tools like `jq` can change.
    Json,
}

#[derive(Debug, Clone, clap::Args)]
struct ExportArgs {
    /// Domain whose records to export (e.g. example.com).
    #[arg(value_name = "DOMAIN", value_parser = crate::idn::parse_name_arg)]
    domain: String,

    /// Output format: `zone` or `json`.
    #[arg(long, value_enum, default_value = "zone", value_name = "FORMAT")]
    format: ExportFormat,

    /// Leave out records a standard DNS provider can't use: a record type the CLI
    /// doesn't know, and A or AAAA records whose value is not an IP address. A zone
    /// file lists what was left out in comments, and either format prints a notice
    /// on stderr.
    #[arg(long)]
    standard_only: bool,
}

/// What to tell whoever ran the export about records `--standard-only` left out, as
/// lines for stderr. Printed whatever the format, since a zone file's comments are
/// not seen by someone writing it to a file and JSON has no comments at all. Empty
/// without the flag (a record that can't be used then stops the export instead) and
/// when nothing was left out.
fn notices(records: &[types::DnsRecord], args: &ExportArgs) -> Vec<String> {
    if !args.standard_only {
        return Vec::new();
    }
    let left_out: Vec<String> = records
        .iter()
        .filter(|r| !is_provider_managed(r) && is_nonstandard(r))
        .map(describe_record)
        .collect();
    if left_out.is_empty() {
        return Vec::new();
    }
    vec![format!(
        "Left out {} record(s) a standard DNS provider can't use: {}. Set them up again with \
         your new provider.",
        left_out.len(),
        left_out.join("; ")
    )]
}

/// Write each notice on its own line, ignoring a failure to write: an export that
/// worked should not fail because stderr is closed.
fn write_notices(out: &mut impl Write, notices: &[String]) {
    for notice in notices {
        let _ = writeln!(out, "Note: {notice}");
    }
}

/// `records` as the text `dns export` prints. The SOA, the domain's own NS records
/// and the parked marker are never exported, in either format: every DNS provider
/// manages its own SOA and NS records, and only GoDaddy can write the marker. With
/// `--standard-only` the records a standard DNS provider can't use are left out too.
fn render(records: Vec<types::DnsRecord>, args: &ExportArgs) -> Result<String, CliCoreError> {
    let records: Vec<types::DnsRecord> = records
        .into_iter()
        .filter(|r| !is_provider_managed(r))
        .collect();
    // Only worth suggesting when some record would have been left out.
    let suggest_standard_only = !args.standard_only && records.iter().any(is_nonstandard);
    let (records, left_out) = if args.standard_only {
        split_nonstandard(&records)
    } else {
        (records, Vec::new())
    };
    match args.format {
        ExportFormat::Zone => export_zone(&records, &args.domain, &left_out).map_err(|errors| {
            crate::error::GddyError::unexpected(format!(
                "{} record(s) could not be written to a zone file:\n  {}",
                errors.len(),
                errors.join("\n  ")
            ))
            .with_fix(if suggest_standard_only {
                format!(
                    "Add `--standard-only` to leave out the records a standard DNS provider \
                     can't use, or run `gddy dns export {} --format json` to export everything \
                     as it is.",
                    args.domain
                )
            } else {
                format!(
                    "Run `gddy dns export {} --format json` to export them as JSON instead.",
                    args.domain
                )
            })
            .into_cli_error()
        }),
        ExportFormat::Json => {
            let serialize = |e: serde_json::Error| {
                crate::error::GddyError::unexpected(format!("failed to serialize DNS records: {e}"))
                    .into_cli_error()
            };
            let list = build_list_output(&records).map_err(serialize)?;
            serde_json::to_string_pretty(&list).map_err(serialize)
        }
    }
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<ExportArgs, _, _, _>(
        CommandSpec::from_args::<ExportArgs>(
            "export",
            "Export a domain's DNS records as a zone file",
        )
        .with_long(
            "Prints every DNS record for a domain as a zone file, in the RFC \
            1035 master file format (also called BIND format) that DNS servers \
            and most DNS providers read, so the zone can be copied to another \
            domain or provider, kept in version control, or restored later with \
            `dns import`. Save it with \
            `gddy dns export example.com > example.com.zone`. Pass \
            `--format json` to use a JSON format instead. The SOA record and \
            the NS records for the domain itself are left out, because every \
            DNS provider manages its own; NS records for subdomains are kept. \
            `ALIAS` or parked domain `A` records are also omitted.",
        )
        .with_system("domain")
        .with_tier(Tier::Read)
        .raw_output(true)
        .with_scopes(&[DOMAINS_READ]),
        |ctx, args: ExportArgs| async move {
            let debug = !ctx.middleware.debug.is_empty();
            let client = make_client(&ctx).await?;
            let records = fetch_records(&client, args.domain.as_str(), None, None, debug).await?;
            let notices = notices(&records, &args);
            let text = render(records, &args)?;
            // After the export succeeded, and on stderr so a redirected file stays
            // clean. Warnings on a raw-output result are never shown, so write here.
            write_notices(&mut std::io::stderr(), &notices);
            Ok(CommandResult::new(json!(text)))
        },
    )
}

#[cfg(test)]
#[path = "export_tests.rs"]
mod tests;
