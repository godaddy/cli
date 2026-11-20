//! `dns import` — add the records in a zone file (or in `dns list -o json`
//! output) to a domain; with `--replace`, make the domain's records match it.

use std::io::IsTerminal as _;

use cli_engine::{
    Alignment, CliCoreError, CommandResult, CommandSpec, NextActionParam, RuntimeCommandSpec,
    TableColumn, Tier,
};
use domains_client::types;
use serde_json::{Value, json};
use tokio::io::AsyncReadExt as _;

use crate::domain::make_client;
use crate::error::GddyError;
use crate::next_action::next_action;
use crate::output_schema::output_schema;
use crate::scopes::DOMAINS_DNS_UPDATE;

use super::failure::{DnsFailure, FailureKind, MutateFailure};
use super::import_plan::{Group, ImportPlan, build_plan, validate};
use super::records::{as_stored, change_columns, change_entry, fetch_records};
use super::set::{ApplyTarget, SetAction, SetOutcome, apply_set_plan, preview_entries};
use super::zone_records::{Imported, Skipped, from_json_text, from_zone_text};

/// The most input `import` reads. A zone is a few hundred records at most; this
/// is far past that and still far below what could exhaust memory.
const MAX_INPUT_BYTES: u64 = 16 * 1024 * 1024;

/// How many problems an error lists before it says how many more there are.
const MAX_LISTED_PROBLEMS: usize = 20;

output_schema!(DnsImportResult {
    "domain": "string";
    "created": "number";
    "replaced": "number";
    "unchanged": "number";
    "deleted": "number";
    "failed": "number";
    "skipped": "number";
    "action": "string";
    "records": "[]object";
});

const DEFAULT_FIELDS: &str =
    "domain,created,replaced,unchanged,deleted,failed,skipped,action,records";

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum InputFormat {
    /// Work it out from the content.
    Auto,
    /// A zone file in the RFC 1035 master file format (also called BIND format).
    Zone,
    /// JSON, as `dns list -o json` or `dns export --format json` writes it.
    Json,
}

#[derive(Debug, Clone, clap::Args)]
struct ImportArgs {
    /// Domain to import the records into (e.g. example.com).
    #[arg(value_name = "DOMAIN", value_parser = crate::idn::parse_name_arg)]
    domain: String,

    /// File to read, or `-` for standard input.
    #[arg(long, value_name = "FILE")]
    file: String,

    /// Format of the input; by default it is worked out from the content.
    #[arg(long, value_enum, default_value = "auto", value_name = "FORMAT")]
    format: InputFormat,

    /// Also delete the domain's records that are not in the input, so the domain
    /// ends up with exactly the records the input has.
    #[arg(long)]
    replace: bool,

    #[command(flatten)]
    domain_format: crate::idn::DomainFormArgs,
}

fn view_columns() -> Vec<TableColumn> {
    let mut nested = vec![
        TableColumn::new("type", "Type"),
        TableColumn::new("name", "Name"),
    ];
    nested.extend(change_columns());
    vec![
        TableColumn::new("domain", "Domain").essential(true),
        TableColumn::new("created", "Created").align(Alignment::Right),
        TableColumn::new("replaced", "Replaced").align(Alignment::Right),
        TableColumn::new("unchanged", "Unchanged").align(Alignment::Right),
        TableColumn::new("deleted", "Deleted").align(Alignment::Right),
        TableColumn::new("failed", "Failed").align(Alignment::Right),
        TableColumn::new("skipped", "Skipped").align(Alignment::Right),
        TableColumn::new("action", "Action"),
        TableColumn::new("records", "Records")
            .hide_when_empty(true)
            .nested(nested),
    ]
}

/// What an import did, or with `--dry-run` would do.
#[derive(Debug)]
pub(super) struct Report {
    pub(super) payload: Value,
    pub(super) warnings: Vec<String>,
}

/// Plan the import of `imported` (already checked by [`parse_input`]) into
/// `domain` and, unless `dry_run`, carry it out. `existing` is the whole zone. An
/// error means the import was refused before anything was written, or something
/// failed while writing.
pub(super) async fn run_import(
    client: &domains_client::Client,
    domain: &str,
    imported: Imported,
    existing: &[types::DnsRecord],
    replace: bool,
    dry_run: bool,
    debug: bool,
) -> Result<Report, CliCoreError> {
    let mut warnings = skipped_warnings(&imported.skipped);
    let ImportPlan {
        groups,
        warnings: plan_warnings,
    } = build_plan(domain, imported.desired, existing, replace)
        .map_err(|problems| input_problems(&problems))?;
    warnings.extend(plan_warnings);
    let skipped = imported.skipped.len();

    if dry_run {
        let payload = preview_payload(domain, &groups, skipped);
        return Ok(Report { payload, warnings });
    }
    let payload = apply_groups(client, domain, groups, skipped, debug).await?;
    Ok(Report { payload, warnings })
}

/// A refused input, with what is wrong and where.
fn input_problems(problems: &[String]) -> CliCoreError {
    let shown = problems
        .iter()
        .take(MAX_LISTED_PROBLEMS)
        .map(|p| format!("  {p}"))
        .collect::<Vec<_>>()
        .join("\n");
    let more = problems.len().saturating_sub(MAX_LISTED_PROBLEMS);
    let tail = if more > 0 {
        format!("\n  ... and {more} more")
    } else {
        String::new()
    };
    GddyError::validation(format!(
        "the input has {} problem(s), so nothing was imported:\n{shown}{tail}",
        problems.len()
    ))
    .with_fix("Correct the input and run the import again.")
    .into_cli_error()
}

/// One line for the apex records GoDaddy manages, and one per delegation.
fn skipped_warnings(skipped: &[Skipped]) -> Vec<String> {
    let mut warnings = Vec::new();
    let apex = skipped.iter().filter(|s| s.name == "@").count();
    if apex > 0 {
        warnings.push(format!(
            "Skipped {apex} NS/SOA record(s) at the domain itself: GoDaddy manages them."
        ));
    }
    for s in skipped.iter().filter(|s| s.name != "@") {
        warnings.push(format!(
            "{}: the {} record for `{}` was not imported. GoDaddy manages NS records, so a \
             delegation of that name to other name servers has to be set up another way.",
            s.at, s.record_type, s.name
        ));
    }
    warnings
}

fn entry_for(group: &Group, mut entry: Value) -> Value {
    entry["type"] = json!(group.record_type);
    entry["name"] = json!(group.name);
    entry
}

/// `records` entries for the creates a group cannot make, each with why.
fn blocked_entries(group: &Group) -> Vec<Value> {
    group
        .blocked
        .iter()
        .map(|(at, reason)| {
            let record = as_stored(&group.desired[*at].record());
            let mut entry = change_entry("fail", None, None, Some(&record));
            entry["error"] = json!(reason);
            entry_for(group, entry)
        })
        .collect()
}

fn count_actions(groups: &[Group], matches: fn(&SetAction) -> bool) -> usize {
    groups
        .iter()
        .flat_map(|g| &g.actions)
        .filter(|a| matches(a))
        .count()
}

fn import_result(counts: [usize; 5], skipped: usize, action: &str, records: Vec<Value>) -> Value {
    let [created, replaced, unchanged, deleted, failed] = counts;
    json!({
        "created": created,
        "replaced": replaced,
        "unchanged": unchanged,
        "deleted": deleted,
        "failed": failed,
        "skipped": skipped,
        "action": if created + replaced + deleted + failed == 0 { "nothing to change" } else { action },
        "records": records,
    })
}

fn preview_payload(domain: &str, groups: &[Group], skipped: usize) -> Value {
    let mut records = Vec::new();
    for group in groups {
        let entries = preview_entries(
            &group.record_type,
            &group.name,
            &group.actions,
            &group.existing,
            &group.opts(),
        );
        records.extend(entries.into_iter().map(|e| entry_for(group, e)));
        records.extend(blocked_entries(group));
    }
    let counts = [
        count_actions(groups, |a| matches!(a, SetAction::Create { .. })),
        count_actions(groups, |a| matches!(a, SetAction::Replace { .. })),
        count_actions(groups, |a| matches!(a, SetAction::Unchanged { .. })),
        count_actions(groups, |a| matches!(a, SetAction::Delete { .. })),
        groups.iter().map(|g| g.blocked.len()).sum(),
    ];
    let mut payload = import_result(counts, skipped, "would import", records);
    payload["domain"] = json!(domain);
    payload
}

/// Run each group's deletes first, across the whole zone, then the rest: removing
/// a record is what lets a record of another type take its name.
async fn apply_groups(
    client: &domains_client::Client,
    domain: &str,
    mut groups: Vec<Group>,
    skipped: usize,
    debug: bool,
) -> Result<Value, CliCoreError> {
    let mut deletes = Vec::with_capacity(groups.len());
    let mut rest = Vec::with_capacity(groups.len());
    for group in &mut groups {
        let (d, r): (Vec<_>, Vec<_>) = std::mem::take(&mut group.actions)
            .into_iter()
            .partition(|a| matches!(a, SetAction::Delete { .. }));
        deletes.push(d);
        rest.push(r);
    }
    let mut outcomes: Vec<Vec<SetOutcome>> = groups.iter().map(|_| Vec::new()).collect();
    for phase in [deletes, rest] {
        for ((group, actions), done) in groups.iter().zip(phase).zip(&mut outcomes) {
            let opts = group.opts();
            let target = ApplyTarget {
                domain,
                name: &group.name,
                record_type: &group.record_type,
                existing: &group.existing,
                opts: &opts,
                replace_conflicting: false,
                debug,
            };
            done.extend(apply_set_plan(client, &target, actions).await);
        }
    }
    summarize(domain, &groups, &outcomes, skipped)
}

/// The result of a real run: `Ok` when every change was made, or an error with a
/// line for each change that was not.
fn summarize(
    domain: &str,
    groups: &[Group],
    outcomes: &[Vec<SetOutcome>],
    skipped: usize,
) -> Result<Value, CliCoreError> {
    let mut records = Vec::new();
    let mut lines = Vec::new();
    let mut failures: Vec<DnsFailure> = Vec::new();
    let mut counts = [0usize; 5];
    let mut total = 0;
    for (group, done) in groups.iter().zip(outcomes) {
        for outcome in done {
            total += 1;
            let mut entry = outcome.change_entry();
            let label = format!("{} {} {}", group.record_type, group.name, outcome.detail);
            match &outcome.error {
                None => {
                    let slot = match outcome.kind {
                        "created" => 0,
                        "replaced" => 1,
                        "unchanged" => 2,
                        _ => 3,
                    };
                    counts[slot] += 1;
                    lines.push(format!("  ✓ {} {label}", outcome.kind));
                }
                Some(error) => {
                    counts[4] += 1;
                    entry["action"] = json!("fail");
                    entry["error"] = json!(error.to_string());
                    lines.push(format!("  ✗ {} {label} — {error}", outcome.kind));
                    failures.push(error.clone());
                }
            }
            records.push(entry_for(group, entry));
        }
        for (at, reason) in &group.blocked {
            total += 1;
            counts[4] += 1;
            let d = &group.desired[*at];
            lines.push(format!("  ✗ created {} — {reason}", d.label()));
            failures.push(DnsFailure::new(FailureKind::RecordExists, reason.clone()));
        }
        records.extend(blocked_entries(group));
    }
    if counts[4] > 0 {
        return Err(super::mutate_failed(MutateFailure::new(
            format!(
                "import into {domain} partly failed — {} of {total} change(s) failed:\n{}\n\n\
                 Run `gddy dns list {domain}` to review the current state. Running the import \
                 again is safe: records already in place are left alone.",
                counts[4],
                lines.join("\n")
            ),
            failures.iter(),
        )));
    }
    let mut payload = import_result(counts, skipped, "import", records);
    payload["domain"] = json!(domain);
    Ok(payload)
}

/// Read the input named by `--file`: a path, or `-` for standard input.
async fn read_input(file: &str) -> Result<String, CliCoreError> {
    let unreadable = |what: String| {
        GddyError::validation(what)
            .with_fix("Check the path, or pipe the records in with `--file -`.")
            .into_cli_error()
    };
    let mut bytes = Vec::new();
    if file == "-" {
        if std::io::stdin().is_terminal() {
            return Err(unreadable(
                "`--file -` reads standard input, but nothing is piped in".to_owned(),
            ));
        }
        tokio::io::stdin()
            .take(MAX_INPUT_BYTES + 1)
            .read_to_end(&mut bytes)
            .await
            .map_err(|e| unreadable(format!("could not read standard input: {e}")))?;
    } else {
        let handle = tokio::fs::File::open(file)
            .await
            .map_err(|e| unreadable(format!("could not open {file}: {e}")))?;
        handle
            .take(MAX_INPUT_BYTES + 1)
            .read_to_end(&mut bytes)
            .await
            .map_err(|e| unreadable(format!("could not read {file}: {e}")))?;
    }
    if bytes.len() as u64 > MAX_INPUT_BYTES {
        return Err(unreadable(format!(
            "the input is larger than {} MiB, which is too big to be a DNS zone",
            MAX_INPUT_BYTES / (1024 * 1024)
        )));
    }
    String::from_utf8(bytes)
        .map(|text| text.trim_start_matches('\u{feff}').to_owned())
        .map_err(|_| unreadable("the input is not valid UTF-8 text".to_owned()))
}

/// Parse `text` as the `format` says, working it out when that is `Auto`, and
/// check every record. Needs no network, so a bad input is refused before login.
fn parse_input(text: &str, format: InputFormat, domain: &str) -> Result<Imported, CliCoreError> {
    let json = match format {
        InputFormat::Json => true,
        InputFormat::Zone => false,
        InputFormat::Auto => text.trim_start().starts_with(['[', '{']),
    };
    let parsed = if json {
        from_json_text(text)
    } else {
        from_zone_text(text, domain)
    };
    let mut imported = parsed.map_err(|problems| input_problems(&problems))?;
    let problems = validate(&mut imported.desired);
    if problems.is_empty() {
        Ok(imported)
    } else {
        Err(input_problems(&problems))
    }
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<ImportArgs, _, _, _>(
        CommandSpec::from_args::<ImportArgs>(
            "import",
            "Import DNS records from a zone file or JSON into a domain",
        )
        .with_long(
            "Reads DNS records from a file (or standard input with `--file -`) \
            and adds them to a domain. The input is a zone file in the RFC 1035 \
            master file format (also called BIND format), as `dns export` writes \
            it, or JSON as `dns list -o json` writes it; the format is worked \
            out from the content. Records the domain already has are left alone, \
            a record that matches except for its TTL gets the new TTL, and \
            nothing is deleted. Pass `--replace` to make the domain's records \
            match the input exactly, which deletes the records that are not in \
            it. NS and SOA records are managed by GoDaddy and are skipped. \
            Every record is checked before anything is written. Pass `--dry-run` \
            to see what would change without writing it.",
        )
        .with_system("domain")
        .with_tier(Tier::Destructive)
        .handles_dry_run(true)
        .with_default_fields(DEFAULT_FIELDS)
        .with_output_schema::<DnsImportResult>()
        .with_view(view_columns())
        .with_scopes(&[DOMAINS_DNS_UPDATE]),
        |ctx, args: ImportArgs| async move {
            let domain_format = args.domain_format.form(&ctx)?;
            let text = read_input(&args.file).await?;
            let imported = parse_input(&text, args.format, &args.domain)?;

            let debug = !ctx.middleware.debug.is_empty();
            let client = make_client(&ctx).await?;
            let existing = fetch_records(&client, args.domain.as_str(), None, None, debug).await?;
            let report = run_import(
                &client,
                &args.domain,
                imported,
                &existing,
                args.replace,
                ctx.dry_run(),
                debug,
            )
            .await?;

            let mut payload = report.payload;
            domain_format.convert_fields(&mut payload, &["domain"]);
            let mut result = CommandResult::new(payload);
            for warning in report.warnings {
                result = result.with_warning(warning);
            }
            if ctx.dry_run() {
                return Ok(result.with_dry_run());
            }
            Ok(result.with_next_actions(vec![
                next_action("dns list <domain>", "Review the domain's records").with_param(
                    "domain",
                    NextActionParam::value(domain_format.render(&args.domain)),
                ),
            ]))
        },
    )
}

#[cfg(test)]
#[path = "import_tests.rs"]
mod tests;
