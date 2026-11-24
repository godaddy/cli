//! `dns set` — replace every record for a type+name pair (destructive).

use cli_engine::{Alignment, CommandResult, CommandSpec, RuntimeCommandSpec, TableColumn, Tier};

use crate::domain::make_client;
use crate::output_schema::output_schema;
use crate::scopes::DOMAINS_DNS_UPDATE;

use super::records::{
    RecordOptions, RecordWriteArgs, ZONE_AND_NAME, change_columns, fetch_records, v3_records,
    validate_applicable_fields, validate_caa_fields, validate_record_name, validate_svcb_fields,
    validate_tlsa_fields, verified_result,
};

mod apply;
mod outcome;
mod plan;
mod verify;
mod write;

use apply::{PlanTarget, apply_plan};
use outcome::{dry_run_set_preview, summarize_set_outcomes};
use plan::plan_set;

// What `dns import` reuses: it makes its own plans, one per type and name, and
// applies and previews them the way `set` does.
pub(in crate::dns) use apply::{PlanTarget as ApplyTarget, apply_plan as apply_set_plan};
pub(in crate::dns) use outcome::{SetOutcome, preview_entries};
pub(in crate::dns) use plan::{SetAction, plan_set as plan_set_actions};
use verify::verify_final_state;

// `dns set` reconciles over the API's per-record ops; it reports how many records it
// replaced, left unchanged, created, and deleted to reach the desired set, and the
// `records` showing what happened to each (the same shape `--dry-run` previews).
output_schema!(DnsSetResult {
    "domain": "string";
    "type": "string";
    "name": "string";
    "replaced": "number";
    "unchanged": "number";
    "created": "number";
    "deleted": "number";
    "action": "string";
    "records": "[]object";
});

/// The fields shown when the caller doesn't pass `--fields`; anything not
/// listed here is dropped from the output.
const DEFAULT_FIELDS: &str = "domain,type,name,replaced,unchanged,created,deleted,action,records";

#[derive(Debug, Clone, clap::Args)]
struct SetArgs {
    #[command(flatten)]
    write: RecordWriteArgs,

    /// Remove any existing CNAME (or, if setting a CNAME, any other type) at
    /// this name first.
    #[arg(long = "replace-conflicting-types")]
    replace_conflicting_types: bool,
}

/// `records` — what happened to each record, or with `--dry-run` what would — renders
/// as an indented child table instead of a raw JSON dump.
fn view_columns() -> Vec<TableColumn> {
    vec![
        TableColumn::new("domain", "Domain"),
        TableColumn::new("type", "Type"),
        TableColumn::new("name", "Name"),
        TableColumn::new("replaced", "Replaced").align(Alignment::Right),
        TableColumn::new("unchanged", "Unchanged").align(Alignment::Right),
        TableColumn::new("created", "Created").align(Alignment::Right),
        TableColumn::new("deleted", "Deleted").align(Alignment::Right),
        TableColumn::new("action", "Action"),
        TableColumn::new("records", "Records")
            .hide_when_empty(true)
            .nested(change_columns()),
    ]
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<SetArgs, _, _, _>(
        CommandSpec::from_args::<SetArgs>(
            "set",
            "Replace all records for a type+name (destructive: overwrites existing)",
        )
        .with_long(
            "Replaces every DNS record for the given type+name pair with the \
            values supplied via `--data`, discarding any records that were \
            there before. This is destructive and irreversible — use \
            `dns list --type <type> --name <name>` to review first. Use `dns add` \
            to append without removing existing records.\n\
            \n\
            DNS only allows a CNAME or other record types at a name, never \
            both. Setting into a name that already has the other kind fails \
            with a specific error naming the conflict; pass \
            `--replace-conflicting-types` to remove the conflicting record(s) \
            automatically and retry.",
        )
        .with_system("domain")
        .with_tier(Tier::Destructive)
        .handles_dry_run(true)
        .with_default_fields(DEFAULT_FIELDS)
        .with_output_schema::<DnsSetResult>()
        .with_view(view_columns())
        .with_scopes(&[DOMAINS_DNS_UPDATE]),
        |ctx, args: SetArgs| async move {
            let opts = RecordOptions::per_record(&args.write)
                .map_err(crate::error::GddyError::validation)?;
            let ttl = args.write.ttl;
            let domain = args.write.domain;
            let record_type = args.write.record_type;
            let name = args.write.name;
            let domain_format = args.write.domain_format.form(&ctx)?;
            let replace_conflicting = args.replace_conflicting_types;
            validate_record_name(&record_type, &name)
                .map_err(crate::error::GddyError::validation)?;
            for record_opts in &opts {
                validate_applicable_fields(&record_type, record_opts)
                    .map_err(crate::error::GddyError::validation)?;
            }
            for record_opts in &opts {
                validate_caa_fields(&record_type, record_opts)
                    .map_err(crate::error::GddyError::validation)?;
            }
            for record_opts in &opts {
                validate_tlsa_fields(&record_type, record_opts)
                    .map_err(crate::error::GddyError::validation)?;
            }
            for record_opts in &opts {
                validate_svcb_fields(&record_type, record_opts)
                    .map_err(crate::error::GddyError::validation)?;
            }
            let data = args.write.data;

            let debug = !ctx.middleware.debug.is_empty();
            let client = make_client(&ctx).await?;

            // Reconcile the current type+name records with the desired --data
            // set over v3's per-record ops (no atomic bulk replace exists).
            let existing = fetch_records(
                &client,
                domain.as_str(),
                Some(&record_type),
                Some(&name),
                debug,
            )
            .await?;
            // Every existing record must have a server id to reconcile against —
            // v3 mutations are keyed by recordId. Bail before touching anything
            // if any lacks one, rather than silently dropping it (which would
            // leave it behind and make `set` add records instead of replacing).
            if existing.iter().any(|r| r.record_id.is_none()) {
                let message = format!(
                    "some existing {record_type} records for {name} were returned without a \
                     recordId, so `set` can't safely replace the full set. Re-run `gddy dns \
                     list {domain} --type {record_type} --name {name}` and adjust in the \
                     control panel if this persists."
                );
                let fix = format!(
                    "Re-run `gddy dns list {domain} --type {record_type} --name {name}` and \
                     adjust in the control panel if this persists."
                );
                return Err(crate::error::GddyError::unexpected(message)
                    .with_fix(fix)
                    .into_cli_error());
            }
            let desired = v3_records(&name, &record_type, &data, &opts);
            let plan = plan_set(&existing, &desired, ttl);
            if ctx.dry_run() {
                let mut preview =
                    dry_run_set_preview(&domain, &record_type, &name, &plan, &existing, &opts);
                domain_format.convert_fields(&mut preview, ZONE_AND_NAME);
                return Ok(CommandResult::new(preview).with_dry_run());
            }

            let target = PlanTarget {
                domain: domain.as_str(),
                name: name.as_str(),
                record_type: record_type.as_str(),
                existing: &existing,
                opts: &opts,
                replace_conflicting,
                debug,
            };
            let outcomes = apply_plan(&client, &target, plan).await;

            let summary = summarize_set_outcomes(&domain, &record_type, &name, &outcomes)
                .map_err(super::mutate_failed)?;
            // The plan came from one earlier read, so confirm the type+name now
            // holds exactly what was asked for before reporting success.
            verify_final_state(
                &client,
                domain.as_str(),
                &record_type,
                &name,
                &desired,
                ttl.is_some(),
                debug,
            )
            .await?;
            Ok(verified_result(
                domain_format,
                summary,
                &domain,
                &record_type,
                &name,
            ))
        },
    )
}
