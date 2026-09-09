//! `dns set` — replace every record for a type+name pair (destructive).

use cli_engine::{Alignment, CommandResult, CommandSpec, RuntimeCommandSpec, TableColumn, Tier};

use crate::domain::{api_error, make_client};
use crate::output_schema::output_schema;
use crate::scopes::DOMAINS_DNS_UPDATE;

use super::records::verify_with_list_action;
use super::records::{RecordOptions, RecordWriteArgs, fetch_records, validate_caa_fields};

mod outcome;
mod plan;
mod write;

use outcome::{SetOutcome, dry_run_set_preview, record_label, summarize_set_outcomes};
use plan::{SetAction, plan_set};
use write::{WriteRequest, apply_replace, write_with_conflict_handling};

// `dns set` reconciles over v3's per-record ops; it reports how many records it
// replaced, created, and deleted to reach the desired set.
output_schema!(DnsSetResult {
    "domain": "string";
    "type": "string";
    "name": "string";
    "replaced": "number";
    "created": "number";
    "deleted": "number";
    "action": "string";
    "plan": "[]object", optional;
});

#[derive(Debug, Clone, clap::Args)]
struct SetArgs {
    #[command(flatten)]
    write: RecordWriteArgs,

    /// Remove any existing CNAME (or, if setting a CNAME, any other type) at
    /// this name first.
    #[arg(long = "replace-conflicting-types")]
    replace_conflicting_types: bool,
}

/// `plan` only appears on the `--dry-run` preview (the real set's success
/// payload has no per-action breakdown), so it renders as an indented child
/// table there instead of a raw JSON dump.
fn view_columns() -> Vec<TableColumn> {
    vec![
        TableColumn::new("domain", "Domain"),
        TableColumn::new("type", "Type"),
        TableColumn::new("name", "Name"),
        TableColumn::new("replaced", "Replaced").align(Alignment::Right),
        TableColumn::new("created", "Created").align(Alignment::Right),
        TableColumn::new("deleted", "Deleted").align(Alignment::Right),
        TableColumn::new("action", "Action"),
        TableColumn::new("plan", "Plan").nested(vec![
            TableColumn::new("action", "Action"),
            TableColumn::new("recordId", "Record ID"),
            TableColumn::new("data", "Data"),
        ]),
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
            there before. v3 has no bulk or in-place replace, so this reconciles \
            over per-record calls: for the overlap it creates the replacement \
            value then deletes the record it's replacing, it deletes the surplus, \
            and it creates the shortfall (so it is not atomic — a mid-run failure \
            is reported per record). This is destructive and irreversible — use \
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
        .with_default_fields("domain,type,name,replaced,created,deleted,action,plan")
        .with_output_schema::<DnsSetResult>()
        .with_view(view_columns())
        .with_scopes(&[DOMAINS_DNS_UPDATE]),
        |ctx, args: SetArgs| async move {
            let opts = RecordOptions::from_write_args(&args.write);
            let domain = args.write.domain;
            let record_type = args.write.record_type;
            let name = args.write.name;
            let data = args.write.data;
            let replace_conflicting = args.replace_conflicting_types;
            validate_caa_fields(&record_type, &opts)
                .map_err(crate::error::GddyError::validation)?;

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
            let existing_ids: Vec<String> = existing
                .iter()
                .filter_map(|r| r.record_id.clone())
                .collect();

            let plan = plan_set(&existing_ids, &data);
            if ctx.dry_run() {
                return Ok(CommandResult::new(dry_run_set_preview(
                    &domain,
                    &record_type,
                    &name,
                    &plan,
                ))
                .with_dry_run());
            }

            let mut outcomes = Vec::new();
            for action in plan {
                match action {
                    SetAction::Replace {
                        record_id,
                        data: value,
                    } => {
                        let req = WriteRequest {
                            domain: domain.as_str(),
                            name: name.as_str(),
                            record_type: record_type.as_str(),
                            value: &value,
                            opts: &opts,
                            replace_conflicting,
                            debug,
                        };
                        let old_record = existing
                            .iter()
                            .find(|r| r.record_id.as_deref() == Some(record_id.as_str()));
                        let old_detail = record_label(&existing, &record_type, &record_id);
                        outcomes.extend(
                            apply_replace(
                                &client,
                                &req,
                                &record_id,
                                &old_detail,
                                old_record.and_then(|r| r.data.as_deref()),
                            )
                            .await,
                        );
                    }
                    SetAction::Create { data: value } => {
                        let req = WriteRequest {
                            domain: domain.as_str(),
                            name: name.as_str(),
                            record_type: record_type.as_str(),
                            value: &value,
                            opts: &opts,
                            replace_conflicting,
                            debug,
                        };
                        outcomes.extend(write_with_conflict_handling(&client, &req).await);
                    }
                    SetAction::Delete { record_id } => {
                        let res = client
                            .delete_dns_record()
                            .zone(domain.as_str())
                            .record_id(record_id.as_str())
                            .send()
                            .await;
                        let err = match res {
                            Ok(_) => None,
                            Err(e) => {
                                Some(api_error("deleting DNS record", debug, e).await.to_string())
                            }
                        };
                        let detail = record_label(&existing, &record_type, &record_id);
                        outcomes.push(SetOutcome::new("deleted", detail, err));
                    }
                }
            }

            summarize_set_outcomes(&domain, &record_type, &name, &outcomes)
                .map(|v| {
                    CommandResult::new(v).with_next_actions(vec![verify_with_list_action(
                        &domain,
                        &record_type,
                        &name,
                    )])
                })
                .map_err(super::mutate_failed)
        },
    )
}
