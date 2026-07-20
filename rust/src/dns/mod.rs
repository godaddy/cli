//! `gddy dns` — manage a domain's DNS records (A, AAAA, CNAME, MX, TXT, …).
//!
//! All commands use the v3 DNS record endpoints under
//! `/v3/domains/zones/{zone}/dns-records`, sharing auth/env resolution with
//! `domain` via [`crate::domain::make_client`]:
//!
//! * **`list`** — `GET` the (paginated) record collection, with optional
//!   `type`/`name` filters.
//! * **`add`** — `POST` one record per `--data` value. `--ttl` is optional on the
//!   CLI (defaults to 3600 when omitted); the v3 record payload itself requires a ttl.
//! * **`set`** / **`delete`** — v3 keys replace/delete on a server-assigned
//!   record id, so these first list the matching records and then act per record
//!   (`set` reconciles: reuse ids, delete surplus, create shortfall). They are
//!   therefore not atomic — a partial failure is reported per record.
//!
//! Reads (`list`) require the `domains.domain:read` scope; mutations (`add`,
//! `set`, `delete`) require `domains.dns:update`. `set` and `delete` are
//! [`Tier::Destructive`] — they overwrite or remove existing records — so
//! `--dry-run` short-circuits them with a preview.

use cli_engine::{
    CliCoreError, CommandContext, CommandResult, CommandSpec, GroupSpec, Module, NextAction,
    NextActionParam, RuntimeCommandSpec, RuntimeGroupSpec, Tier,
};
use serde_json::{Value, json};

use crate::domain::{api_error, make_client, string_list};
use crate::next_action::next_action;
use crate::output_schema::output_schema;
use crate::scopes::{DOMAINS_DNS_UPDATE, DOMAINS_READ};

use domains_client::types;

// `dns add` creates one v3 record per `--data` value; it reports each outcome
// individually so a partial failure is explicit (see the handler).
output_schema!(DnsAddResult {
    "domain": "string";
    "type": "string";
    "name": "string";
    "created": "number";
    "failed": "number";
    "results": "[]object";
    "action": "string";
});

// `dns set` reconciles over v3's per-record ops; it reports how many records it
// reused (replaced), created, and deleted to reach the desired set.
output_schema!(DnsSetResult {
    "domain": "string";
    "type": "string";
    "name": "string";
    "replaced": "number";
    "created": "number";
    "deleted": "number";
    "action": "string";
});

output_schema!(DnsDeleteResult {
    "domain": "string";
    "type": "string";
    "name": "string";
    "deleted": "number";
    "failed": "number";
    "action": "string";
});

/// DNS record types the CLI can create/replace/delete via v3 (`add`/`set`/`delete`).
/// NS and SOA are registry-managed / read-only, so they're excluded. v3's
/// `DNSRecordType` is otherwise an open string; this list is the CLI's guardrail
/// against typos and includes `CAA` and GoDaddy's `ALIAS` extension.
const WRITABLE_TYPES: &[&str] = &["A", "AAAA", "ALIAS", "CAA", "CNAME", "MX", "SRV", "TXT"];
/// Record types accepted by the `list` filter — the writable set plus the
/// read-only NS/SOA, which are listable even though they can't be modified.
const LISTABLE_TYPES: &[&str] = &[
    "A", "AAAA", "ALIAS", "CAA", "CNAME", "MX", "NS", "SOA", "SRV", "TXT",
];
/// Default TTL (seconds) for `dns add`/`set` when `--ttl` is omitted (v3 requires a ttl).
const DEFAULT_TTL: i64 = 3600;
/// Page size for the paginated v3 list; the handler pages through until every
/// matching record is collected.
const LIST_PAGE_SIZE: i64 = 100;

fn arg_str(ctx: &CommandContext, key: &str) -> Option<String> {
    ctx.args
        .get(key)
        .and_then(|v| v.as_str())
        .map(str::to_owned)
}

/// clap value-parser for a mutating `--type` (`add`/`set`/`delete`): validate
/// against [`WRITABLE_TYPES`] and return the canonical upper-case wire string.
/// The read-only NS/SOA get a clear "managed by GoDaddy" reason. Validating in
/// clap rejects invalid input at parse time — before `--dry-run`/auth short-circuits.
fn parse_write_type_arg(raw: &str) -> Result<String, String> {
    let upper = raw.to_ascii_uppercase();
    if WRITABLE_TYPES.contains(&upper.as_str()) {
        return Ok(upper);
    }
    if matches!(upper.as_str(), "NS" | "SOA") {
        Err(format!(
            "{upper} records are managed by GoDaddy and can't be created, replaced, or deleted; \
             writable types: {}",
            WRITABLE_TYPES.join(", ")
        ))
    } else {
        Err(format!(
            "invalid record type {raw:?}; expected one of {}",
            WRITABLE_TYPES.join(", ")
        ))
    }
}

/// clap value-parser for the `list` `--type` filter: [`LISTABLE_TYPES`] (the
/// writable set plus read-only NS/SOA), upper-cased.
fn parse_list_type_arg(raw: &str) -> Result<String, String> {
    let upper = raw.to_ascii_uppercase();
    if LISTABLE_TYPES.contains(&upper.as_str()) {
        Ok(upper)
    } else {
        Err(format!(
            "invalid record type {raw:?}; expected one of {}",
            LISTABLE_TYPES.join(", ")
        ))
    }
}

/// Optional record fields shared by `add` and `set`, including the CAA-only
/// `flag`/`tag`.
struct RecordOptions {
    ttl: Option<i64>,
    priority: Option<i64>,
    port: Option<i64>,
    weight: Option<i64>,
    protocol: Option<String>,
    service: Option<String>,
    flag: Option<i64>,
    tag: Option<String>,
}

impl RecordOptions {
    fn from_ctx(ctx: &CommandContext) -> Self {
        let as_i64 = |key: &str| ctx.args.get(key).and_then(Value::as_i64);
        RecordOptions {
            ttl: as_i64("ttl"),
            priority: as_i64("priority"),
            port: as_i64("port"),
            weight: as_i64("weight"),
            protocol: arg_str(ctx, "protocol"),
            service: arg_str(ctx, "service"),
            flag: as_i64("flag"),
            tag: arg_str(ctx, "tag"),
        }
    }
}

/// Validate the CAA-specific fields against the record type. A CAA record needs a
/// `--tag` (`--data` carries the CA domain / value); `--flag`/`--tag` are
/// meaningless for other types. Pure so it's unit-testable and runs before any
/// network call.
fn validate_caa_fields(record_type: &str, opts: &RecordOptions) -> Result<(), String> {
    if record_type == "CAA" {
        if opts.tag.as_deref().map(str::trim).unwrap_or("").is_empty() {
            return Err(
                "CAA records require --tag (e.g. issue, issuewild, iodef); --data carries the \
                 CA domain / value"
                    .to_string(),
            );
        }
    } else if opts.flag.is_some() || opts.tag.is_some() {
        return Err(format!(
            "--flag/--tag are only valid for CAA records, not {record_type}"
        ));
    }
    Ok(())
}

/// Build one v3 `DnsRecord` from a `--data` value + shared options. `ttl` defaults
/// to [`DEFAULT_TTL`] (v3 requires it). SRV/MX numerics convert into v3's `u16`
/// and the CAA `flag` into `u8` (clap already bounds both ranges).
fn v3_record(name: &str, ty: &str, data: &str, opts: &RecordOptions) -> types::DnsRecord {
    let to_u16 = |v: Option<i64>| v.and_then(|n| u16::try_from(n).ok());
    types::DnsRecord {
        data: data.to_owned(),
        flag: opts.flag.and_then(|n| u8::try_from(n).ok()),
        name: name.to_owned(),
        port: to_u16(opts.port),
        priority: to_u16(opts.priority),
        protocol: opts.protocol.clone(),
        record_id: None,
        service: opts.service.clone(),
        tag: opts.tag.clone(),
        ttl: opts.ttl.unwrap_or(DEFAULT_TTL),
        type_: types::DnsRecordType(ty.to_owned()),
        weight: to_u16(opts.weight),
    }
}

/// Build the v3 `DnsRecord`s for `add` — one per `--data` value (parallel to it).
fn v3_records(
    name: &str,
    ty: &str,
    data: &[String],
    opts: &RecordOptions,
) -> Vec<types::DnsRecord> {
    data.iter().map(|d| v3_record(name, ty, d, opts)).collect()
}

/// One reconcile action for `dns set` over v3's per-record endpoints.
#[derive(Debug, PartialEq)]
enum SetAction {
    /// Reuse an existing record's id, replacing its value with a new `--data`.
    Replace { record_id: String, data: String },
    /// Remove an existing record no longer wanted.
    Delete { record_id: String },
    /// Create a record for a surplus `--data` value.
    Create { data: String },
}

/// Reconcile the existing records for a type+name (their ids, in list order) with
/// the desired `--data` values: reuse ids for the overlap (`Replace`), `Delete`
/// the surplus existing, `Create` the surplus desired. Pure so the plan — the
/// least-destructive way to emulate a set-replace over per-record v3 ops — is
/// unit-testable.
fn plan_set(existing_ids: &[String], desired: &[String]) -> Vec<SetAction> {
    let overlap = existing_ids.len().min(desired.len());
    let mut actions = Vec::with_capacity(existing_ids.len().max(desired.len()));
    for i in 0..overlap {
        actions.push(SetAction::Replace {
            record_id: existing_ids[i].clone(),
            data: desired[i].clone(),
        });
    }
    for id in &existing_ids[overlap..] {
        actions.push(SetAction::Delete {
            record_id: id.clone(),
        });
    }
    for d in &desired[overlap..] {
        actions.push(SetAction::Create { data: d.clone() });
    }
    actions
}

/// Outcome of one applied `set` reconcile action, for reporting.
struct SetOutcome {
    /// What was attempted: `"replaced"`, `"created"`, or `"deleted"`.
    kind: &'static str,
    /// The `--data` value (replace/create) or record id (delete).
    detail: String,
    /// `Some(msg)` if the call failed.
    error: Option<String>,
}

impl SetOutcome {
    fn new(kind: &'static str, detail: String, error: Option<String>) -> Self {
        Self {
            kind,
            detail,
            error,
        }
    }
}

/// Build the `set` result from the applied reconcile outcomes: `Ok(json)` with
/// replaced/created/deleted counts when every action succeeded, or `Err(message)`
/// — a non-zero exit — with a per-action ✓/✗ breakdown if any failed. Pure so the
/// success/failure decision and tallies are unit-testable.
fn summarize_set_outcomes(
    domain: &str,
    record_type: &str,
    name: &str,
    outcomes: &[SetOutcome],
) -> Result<Value, String> {
    let failed = outcomes.iter().filter(|o| o.error.is_some()).count();
    let count = |k: &str| {
        outcomes
            .iter()
            .filter(|o| o.error.is_none() && o.kind == k)
            .count()
    };
    let (replaced, created, deleted) = (count("replaced"), count("created"), count("deleted"));

    if failed > 0 {
        let breakdown = outcomes
            .iter()
            .map(|o| match &o.error {
                None => format!("  ✓ {} {}", o.kind, o.detail),
                Some(e) => format!("  ✗ {} {} — {e}", o.kind, o.detail),
            })
            .collect::<Vec<_>>()
            .join("\n");
        return Err(format!(
            "set for {name} ({record_type}) partially failed — {failed} of {} action(s):\n\
             {breakdown}\n\nRe-run `gddy dns set`, or `gddy dns list {domain} --type \
             {record_type} --name {name}` to review the current state.",
            outcomes.len(),
        ));
    }

    Ok(json!({
        "domain": domain,
        "type": record_type,
        "name": name,
        "replaced": replaced,
        "created": created,
        "deleted": deleted,
        "action": "set",
    }))
}

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
             {breakdown}\n\nRe-run `gddy dns delete`, or `gddy dns list {domain} --type \
             {record_type} --name {name}` to review the current state.",
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

/// Next action pointing back at `dns list` to verify a write, pre-filled with
/// the domain/type/name the write just touched.
fn verify_with_list_action(domain: &str, record_type: &str, name: &str) -> NextAction {
    next_action(
        "dns list <domain> --type <type> --name <name>",
        "Verify the records for this type+name",
    )
    .with_param("domain", NextActionParam::value(domain))
    .with_param("type", NextActionParam::value(record_type))
    .with_param("name", NextActionParam::value(name))
}

/// Summarize the per-record create outcomes of `dns add` (each `--data` value
/// paired with `Ok(())` or an `Err(message)`), preserving input order. Returns
/// the success JSON payload when *every* record was created, or an error message
/// (a non-zero exit) with a per-record breakdown if any failed. Pure — no I/O —
/// so the aggregation and the success/failure decision are unit-testable.
fn summarize_add_outcomes(
    domain: &str,
    record_type: &str,
    name: &str,
    outcomes: Vec<(String, Result<(), String>)>,
) -> Result<Value, String> {
    let mut results = Vec::with_capacity(outcomes.len());
    let mut failed = 0usize;
    for (value, outcome) in &outcomes {
        match outcome {
            Ok(()) => results.push(json!({ "data": value, "status": "created" })),
            Err(err) => {
                failed += 1;
                results.push(json!({ "data": value, "status": "failed", "error": err }));
            }
        }
    }
    let total = results.len();
    let created = total - failed;

    if failed > 0 {
        let breakdown = outcomes
            .iter()
            .map(|(value, outcome)| match outcome {
                Ok(()) => format!("  ✓ {value} — created"),
                Err(err) => format!("  ✗ {value} — {err}"),
            })
            .collect::<Vec<_>>()
            .join("\n");
        return Err(format!(
            "added {created} of {total} DNS record(s) for {name} ({record_type}); {failed} \
             failed:\n{breakdown}\n\nRe-run `gddy dns add` with just the failed value(s), or \
             `gddy dns list {domain} --type {record_type} --name {name}` to review the current \
             state."
        ));
    }

    Ok(json!({
        "domain": domain,
        "type": record_type,
        "name": name,
        "created": created,
        "failed": failed,
        "results": results,
        "action": "add",
    }))
}

/// List every v3 DNS record for a zone matching the optional `type`/`name`
/// filters, paging through the collection (v3 list is paginated). Shared by
/// `list`, `set`, and `delete` — the latter two need the matching records' ids.
async fn fetch_records(
    client: &domains_client::Client,
    zone: &str,
    type_: Option<&str>,
    name: Option<&str>,
    debug: bool,
) -> Result<Vec<types::DnsRecord>, CliCoreError> {
    let page_size = std::num::NonZeroU64::new(LIST_PAGE_SIZE as u64)
        .expect("LIST_PAGE_SIZE is a positive constant");
    let mut all = Vec::new();
    let mut page: u64 = 1;
    loop {
        let page_nz = std::num::NonZeroU64::new(page).expect("page starts at 1 and only grows");
        let mut req = client
            .list_dns_records()
            .zone(zone)
            .page(page_nz)
            .page_size(page_size)
            .total_required(true);
        if let Some(t) = type_ {
            req = req.type_(types::DnsRecordType(t.to_owned()));
        }
        if let Some(n) = name {
            req = req.name(n);
        }
        let body = match req.send().await {
            Ok(r) => r.into_inner(),
            Err(e) => return Err(api_error("listing DNS records", debug, e).await),
        };
        let items = body.items.unwrap_or_default();
        let got = items.len();
        all.extend(items);
        // Last page when the API's total_pages is reached, or (absent that) a
        // short/empty page came back.
        let last = body
            .total_pages
            .map(|tp| page >= tp.get())
            .unwrap_or(got < LIST_PAGE_SIZE as usize);
        if last || got == 0 {
            break;
        }
        page += 1;
    }
    Ok(all)
}

/// Shared flags for the mutating commands (`add`/`set`): required type/name and
/// the repeatable `--data`, plus the optional record fields.
fn with_record_write_args(spec: CommandSpec) -> CommandSpec {
    spec.with_arg(
        clap::Arg::new("domain")
            .value_name("DOMAIN")
            .required(true)
            .help("Domain whose records to modify (e.g. example.com)"),
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
            .help("Record name relative to the domain (e.g. www, @ for the apex)"),
    )
    .with_arg(
        clap::Arg::new("data")
            .long("data")
            .value_name("VALUE")
            .required(true)
            .action(clap::ArgAction::Append)
            .help("Record value (repeatable for multiple records on the same name)"),
    )
    .with_arg(
        clap::Arg::new("ttl")
            .long("ttl")
            .value_name("SECONDS")
            .value_parser(clap::value_parser!(i64).range(1..))
            .help("Time-to-live in seconds (defaults to 3600 when omitted)"),
    )
    .with_arg(
        clap::Arg::new("priority")
            .long("priority")
            .value_name("N")
            .value_parser(clap::value_parser!(i64).range(0..=65535))
            .help("Record priority (MX and SRV only)"),
    )
    .with_arg(
        clap::Arg::new("port")
            .long("port")
            .value_name("PORT")
            .value_parser(clap::value_parser!(i64).range(1..=65535))
            .help("Service port (SRV only)"),
    )
    .with_arg(
        clap::Arg::new("weight")
            .long("weight")
            .value_name("N")
            .value_parser(clap::value_parser!(i64).range(0..=65535))
            .help("Record weight (SRV only)"),
    )
    .with_arg(
        clap::Arg::new("protocol")
            .long("protocol")
            .value_name("PROTO")
            .help("Service protocol (SRV only)"),
    )
    .with_arg(
        clap::Arg::new("service")
            .long("service")
            .value_name("SERVICE")
            .help("Service type (SRV only)"),
    )
    .with_arg(
        clap::Arg::new("flag")
            .long("flag")
            .value_name("N")
            .value_parser(clap::value_parser!(i64).range(0..=255))
            .help("CAA flag byte, 0-255 (CAA only; 0 non-critical, 128 critical)"),
    )
    .with_arg(
        clap::Arg::new("tag")
            .long("tag")
            .value_name("TAG")
            // A CAA record needs a tag; enforce it at parse time (before auth).
            // The reverse guard — flag/tag only valid for CAA — lives in the
            // handler (`validate_caa_fields`), which clap can't express.
            .required_if_eq("type", "CAA")
            .help("CAA property tag, e.g. issue/issuewild/iodef (CAA only; required for CAA)"),
    )
}

pub fn module() -> Module {
    Module::new("DNS", |_ctx| {
        RuntimeGroupSpec::new(
            GroupSpec::new("dns", "Manage a domain's DNS records").with_long(
                "Create, read, and delete DNS records for a GoDaddy-managed domain. \
                 `add` appends new records without touching existing ones; `set` replaces \
                 every record for a given type+name pair; `delete` removes all records for \
                 a type+name pair. `set` and `delete` are destructive; pass `--dry-run` to \
                 preview the change without writing it.",
            ),
        )
        // --- list (v3) --------------------------------------------------
        .with_command(RuntimeCommandSpec::new_with_context(
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
                        .help("Only records of this type (A, AAAA, ALIAS, CAA, CNAME, MX, NS, SOA, SRV, TXT)"),
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
        ))
        // --- add (v3) ---------------------------------------------------
        .with_command(RuntimeCommandSpec::new_with_context(
            with_record_write_args(
                CommandSpec::new(
                    "add",
                    "Add DNS records to a domain (appends; non-destructive)",
                )
                .with_long(
                    "Appends one or more DNS records to a domain without modifying any \
                         existing records. Pass `--data` once per record value to add \
                         multiple records for the same type+name (each is a separate v3 \
                         create call). `--ttl` defaults to 3600 when omitted. Use `dns set` \
                         to replace the full record set for a type+name.",
                )
                .with_system("domain")
                .with_tier(Tier::Mutate)
                .with_default_fields("domain,type,name,created,failed")
                .with_output_schema::<DnsAddResult>()
                .with_scopes(&[DOMAINS_DNS_UPDATE]),
            ),
            |ctx| async move {
                let domain = arg_str(&ctx, "domain").unwrap_or_default();
                let record_type = arg_str(&ctx, "type").unwrap_or_default();
                let name = arg_str(&ctx, "name").unwrap_or_default();
                let data = string_list(&ctx, "data");
                let opts = RecordOptions::from_ctx(&ctx);
                validate_caa_fields(&record_type, &opts).map_err(CliCoreError::message)?;
                let records = v3_records(&name, &record_type, &data, &opts);

                let debug = !ctx.middleware.debug.is_empty();
                let client = make_client(&ctx).await?;
                // v3 creates a single record per call. Attempt every record —
                // don't stop at the first failure — and record each outcome, so a
                // partial failure is explicit rather than leaving the user unsure
                // which of the records were actually created. `data` and `records`
                // are parallel (one record per `--data` value).
                let mut outcomes = Vec::with_capacity(records.len());
                for (value, record) in data.iter().zip(records) {
                    let outcome = match client
                        .create_dns_record()
                        .zone(domain.as_str())
                        .body(record)
                        .send()
                        .await
                    {
                        Ok(_) => Ok(()),
                        Err(e) => Err(api_error("adding DNS record", debug, e).await.to_string()),
                    };
                    outcomes.push((value.clone(), outcome));
                }

                // All-created → success payload; any failure → non-zero error
                // with a per-record breakdown.
                summarize_add_outcomes(&domain, &record_type, &name, outcomes)
                    .map(|v| {
                        CommandResult::new(v).with_next_actions(vec![verify_with_list_action(
                            &domain,
                            &record_type,
                            &name,
                        )])
                    })
                    .map_err(CliCoreError::message)
            },
        ))
        // --- set (v3) ---------------------------------------------------
        .with_command(RuntimeCommandSpec::new_with_context(
            with_record_write_args(
                CommandSpec::new(
                    "set",
                    "Replace all records for a type+name (destructive: overwrites existing)",
                )
                .with_long(
                    "Replaces every DNS record for the given type+name pair with the \
                     values supplied via `--data`, discarding any records that were \
                     there before. v3 has no bulk replace, so this reconciles over \
                     per-record calls: it reuses existing records for the overlap, \
                     deletes the surplus, and creates the shortfall (so it is not \
                     atomic — a mid-run failure is reported per record). This is \
                     destructive and irreversible — use `dns list --type <type> \
                     --name <name>` to review first. Use `dns add` to append without \
                     removing existing records.",
                )
                .with_system("domain")
                .with_tier(Tier::Destructive)
                .with_default_fields("domain,type,name,replaced,created,deleted")
                .with_output_schema::<DnsSetResult>()
                .with_scopes(&[DOMAINS_DNS_UPDATE]),
            ),
            |ctx| async move {
                let domain = arg_str(&ctx, "domain").unwrap_or_default();
                let record_type = arg_str(&ctx, "type").unwrap_or_default();
                let name = arg_str(&ctx, "name").unwrap_or_default();
                let data = string_list(&ctx, "data");
                let opts = RecordOptions::from_ctx(&ctx);
                validate_caa_fields(&record_type, &opts).map_err(CliCoreError::message)?;

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
                    return Err(CliCoreError::message(format!(
                        "some existing {record_type} records for {name} were returned without a \
                         recordId, so `set` can't safely replace the full set. Re-run `gddy dns \
                         list {domain} --type {record_type} --name {name}` and adjust in the \
                         control panel if this persists."
                    )));
                }
                let existing_ids: Vec<String> =
                    existing.iter().filter_map(|r| r.record_id.clone()).collect();

                let mut outcomes = Vec::new();
                for action in plan_set(&existing_ids, &data) {
                    let outcome = match &action {
                        SetAction::Replace { record_id, data: value } => {
                            let body = v3_record(&name, &record_type, value, &opts);
                            let res = client
                                .replace_dns_record()
                                .zone(domain.as_str())
                                .record_id(record_id.as_str())
                                .body(body)
                                .send()
                                .await;
                            let err = match res {
                                Ok(_) => None,
                                Err(e) => {
                                    Some(api_error("replacing DNS record", debug, e).await.to_string())
                                }
                            };
                            SetOutcome::new("replaced", value.clone(), err)
                        }
                        SetAction::Create { data: value } => {
                            let body = v3_record(&name, &record_type, value, &opts);
                            let res = client
                                .create_dns_record()
                                .zone(domain.as_str())
                                .body(body)
                                .send()
                                .await;
                            let err = match res {
                                Ok(_) => None,
                                Err(e) => Some(api_error("creating DNS record", debug, e).await.to_string()),
                            };
                            SetOutcome::new("created", value.clone(), err)
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
                                Err(e) => Some(api_error("deleting DNS record", debug, e).await.to_string()),
                            };
                            SetOutcome::new("deleted", record_id.clone(), err)
                        }
                    };
                    outcomes.push(outcome);
                }

                summarize_set_outcomes(&domain, &record_type, &name, &outcomes)
                    .map(|v| {
                        CommandResult::new(v).with_next_actions(vec![verify_with_list_action(
                            &domain,
                            &record_type,
                            &name,
                        )])
                    })
                    .map_err(CliCoreError::message)
            },
        ))
        // --- delete (v3) ------------------------------------------------
        .with_command(RuntimeCommandSpec::new_with_context(
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
                        None => Some(
                            "the API returned this record without a recordId, so it can't be \
                             deleted; re-run `gddy dns list` and remove it in the control panel \
                             if it persists"
                                .to_string(),
                        ),
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
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cli_engine::{Cli, CliConfig};

    fn opts() -> RecordOptions {
        RecordOptions {
            ttl: None,
            priority: None,
            port: None,
            weight: None,
            protocol: None,
            service: None,
            flag: None,
            tag: None,
        }
    }

    #[test]
    fn parse_write_type_arg_accepts_writable_incl_caa_alias_rejects_ns_soa() {
        assert_eq!(parse_write_type_arg("aaaa").expect("valid"), "AAAA");
        assert_eq!(parse_write_type_arg("caa").expect("valid"), "CAA");
        assert_eq!(parse_write_type_arg("Alias").expect("valid"), "ALIAS");
        // NS/SOA are registry-managed / read-only → rejected with a clear reason.
        for ty in ["NS", "soa"] {
            let err = parse_write_type_arg(ty).expect_err("read-only");
            assert!(err.contains("managed by GoDaddy"), "got: {err}");
        }
        let err = parse_write_type_arg("bogus").expect_err("should reject");
        assert!(err.contains("invalid record type"), "got: {err}");
    }

    #[test]
    fn parse_list_type_arg_also_accepts_ns_soa() {
        assert_eq!(parse_list_type_arg("caa").expect("valid"), "CAA");
        assert_eq!(parse_list_type_arg("ns").expect("valid"), "NS");
        assert_eq!(parse_list_type_arg("SOA").expect("valid"), "SOA");
        assert!(parse_list_type_arg("bogus").is_err());
    }

    #[test]
    fn v3_records_builds_one_per_data_value_with_default_ttl() {
        let recs = v3_records(
            "www",
            "A",
            &["1.2.3.4".to_string(), "5.6.7.8".to_string()],
            &opts(),
        );
        assert_eq!(recs.len(), 2);
        assert_eq!(recs[0].name, "www");
        assert_eq!(recs[0].type_.as_str(), "A");
        assert_eq!(recs[0].data, "1.2.3.4");
        // v3 requires a ttl; an omitted --ttl falls back to the default.
        assert_eq!(recs[0].ttl, DEFAULT_TTL);
        assert_eq!(recs[1].data, "5.6.7.8");
    }

    #[test]
    fn v3_record_carries_srv_and_caa_fields() {
        let mut srv_opts = opts();
        srv_opts.ttl = Some(600);
        srv_opts.priority = Some(10);
        srv_opts.port = Some(443);
        srv_opts.weight = Some(5);
        let srv = v3_record("_sip", "SRV", "sip.example.com", &srv_opts);
        assert_eq!(srv.ttl, 600);
        assert_eq!(srv.priority, Some(10));
        assert_eq!(srv.port, Some(443));
        assert_eq!(srv.weight, Some(5));

        let mut caa_opts = opts();
        caa_opts.flag = Some(128);
        caa_opts.tag = Some("issue".to_string());
        let caa = v3_record("@", "CAA", "letsencrypt.org", &caa_opts);
        assert_eq!(caa.flag, Some(128));
        assert_eq!(caa.tag.as_deref(), Some("issue"));
    }

    #[test]
    fn caa_fields_are_required_for_caa_and_rejected_otherwise() {
        // CAA without --tag → rejected.
        let mut caa_no_tag = opts();
        caa_no_tag.flag = Some(0);
        let err = validate_caa_fields("CAA", &caa_no_tag).expect_err("CAA needs a tag");
        assert!(err.contains("--tag"), "got: {err}");
        // CAA with --tag → ok.
        let mut caa = opts();
        caa.tag = Some("issue".to_string());
        assert!(validate_caa_fields("CAA", &caa).is_ok());
        // --flag/--tag on a non-CAA type → rejected.
        let mut a = opts();
        a.tag = Some("issue".to_string());
        let err = validate_caa_fields("A", &a).expect_err("flag/tag are CAA-only");
        assert!(err.contains("only valid for CAA"), "got: {err}");
        // A non-CAA type with no CAA fields → ok.
        assert!(validate_caa_fields("A", &opts()).is_ok());
    }

    #[test]
    fn plan_set_reuses_overlap_deletes_extra_creates_shortfall() {
        // 3 existing, 2 desired → reuse 2 ids, delete the 3rd.
        let existing = vec!["r1".to_string(), "r2".to_string(), "r3".to_string()];
        let desired = vec!["9.9.9.9".to_string(), "8.8.8.8".to_string()];
        assert_eq!(
            plan_set(&existing, &desired),
            vec![
                SetAction::Replace {
                    record_id: "r1".into(),
                    data: "9.9.9.9".into()
                },
                SetAction::Replace {
                    record_id: "r2".into(),
                    data: "8.8.8.8".into()
                },
                SetAction::Delete {
                    record_id: "r3".into()
                },
            ]
        );
        // 1 existing, 3 desired → reuse 1 id, create 2.
        assert_eq!(
            plan_set(
                &["r1".to_string()],
                &["a".to_string(), "b".to_string(), "c".to_string()]
            ),
            vec![
                SetAction::Replace {
                    record_id: "r1".into(),
                    data: "a".into()
                },
                SetAction::Create { data: "b".into() },
                SetAction::Create { data: "c".into() },
            ]
        );
        // none existing → all creates.
        assert_eq!(
            plan_set(&[], &["x".to_string()]),
            vec![SetAction::Create { data: "x".into() }]
        );
    }

    #[test]
    fn summarize_set_reports_counts_and_flags_partial_failure() {
        let all_ok = vec![
            SetOutcome::new("replaced", "9.9.9.9".into(), None),
            SetOutcome::new("created", "8.8.8.8".into(), None),
            SetOutcome::new("deleted", "r3".into(), None),
        ];
        let v = summarize_set_outcomes("example.com", "A", "www", &all_ok).expect("all ok");
        assert_eq!(v["replaced"], 1);
        assert_eq!(v["created"], 1);
        assert_eq!(v["deleted"], 1);

        let mixed = vec![
            SetOutcome::new("replaced", "9.9.9.9".into(), None),
            SetOutcome::new("created", "8.8.8.8".into(), Some("422 bad".into())),
        ];
        let err = summarize_set_outcomes("example.com", "A", "www", &mixed).expect_err("a failure");
        assert!(err.contains("1 of 2"), "{err}");
        assert!(err.contains("✗ created 8.8.8.8 — 422 bad"), "{err}");
    }

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
    }

    /// Type/flag validation lives in clap value-parsers, so invalid input is
    /// rejected at parse time — before auth or `--dry-run` can short-circuit.
    #[tokio::test]
    async fn invalid_dns_input_is_rejected_before_auth_or_dry_run() {
        let cli = || {
            Cli::new(
                CliConfig::new("gddy", "GoDaddy developer CLI", "gddy")
                    .with_default_auth_provider("godaddy")
                    .with_module(module()),
            )
        };
        let cases: [(&[&str], &str); 3] = [
            (
                &[
                    "gddy",
                    "dns",
                    "delete",
                    "example.com",
                    "--type",
                    "NS",
                    "--name",
                    "@",
                ],
                "managed by GoDaddy",
            ),
            (
                &[
                    "gddy",
                    "dns",
                    "set",
                    "example.com",
                    "--type",
                    "BOGUS",
                    "--name",
                    "@",
                    "--data",
                    "x",
                ],
                "invalid record type",
            ),
            (
                &["gddy", "dns", "list", "example.com", "--name", "www"],
                "--type",
            ),
        ];
        for (args, needle) in cases {
            let output = cli().run(args.iter().copied()).await;
            assert_ne!(
                output.exit_code, 0,
                "{args:?} should fail: {}",
                output.rendered
            );
            assert!(
                output.rendered.contains(needle),
                "{args:?} expected {needle:?}, got: {}",
                output.rendered
            );
        }
    }

    /// Like `domain`, the `dns` commands hit the Domains API and must stay
    /// fail-closed at auth resolution (exit code 2). Running each leaf also
    /// exercises clap's command-tree construction (duplicate-subcommand panics
    /// surface only in debug builds).
    #[tokio::test]
    async fn dns_commands_require_auth() {
        const AUTH_FAILURE_EXIT: i32 = 2;
        let cases: [&[&str]; 4] = [
            &["gddy", "dns", "list", "example.com", "--output", "json"],
            &[
                "gddy",
                "dns",
                "add",
                "example.com",
                "--type",
                "A",
                "--name",
                "www",
                "--data",
                "1.2.3.4",
                "--output",
                "json",
            ],
            &[
                "gddy",
                "dns",
                "set",
                "example.com",
                "--type",
                "A",
                "--name",
                "www",
                "--data",
                "5.6.7.8",
                "--output",
                "json",
            ],
            &[
                "gddy",
                "dns",
                "delete",
                "example.com",
                "--type",
                "A",
                "--name",
                "www",
                "--output",
                "json",
            ],
        ];
        for args in cases {
            let cli = Cli::new(
                CliConfig::new("gddy", "GoDaddy developer CLI", "gddy")
                    .with_default_auth_provider("godaddy")
                    .with_module(module()),
            );
            let output = cli.run(args.iter().copied()).await;
            assert_eq!(
                output.exit_code, AUTH_FAILURE_EXIT,
                "{args:?} must fail closed at auth resolution, got: {}",
                output.rendered
            );
        }
    }

    #[test]
    fn dns_add_all_created_reports_each_record() {
        let payload = summarize_add_outcomes(
            "example.com",
            "A",
            "www",
            vec![
                ("1.2.3.4".to_string(), Ok(())),
                ("5.6.7.8".to_string(), Ok(())),
            ],
        )
        .expect("all created -> success payload");
        assert_eq!(payload["created"], 2);
        assert_eq!(payload["failed"], 0);
        let results = payload["results"].as_array().expect("results array");
        assert_eq!(results.len(), 2);
        assert_eq!(results[0]["data"], "1.2.3.4");
        assert_eq!(results[0]["status"], "created");
        assert_eq!(results[1]["data"], "5.6.7.8");
    }

    #[test]
    fn dns_add_partial_failure_is_an_error_with_per_record_breakdown() {
        // Middle record fails: the command must NOT succeed, and the message must
        // make clear which values were created and which failed (and why).
        let err = summarize_add_outcomes(
            "example.com",
            "A",
            "www",
            vec![
                ("1.2.3.4".to_string(), Ok(())),
                ("5.6.7.8".to_string(), Err("422 invalid data".to_string())),
                ("9.9.9.9".to_string(), Ok(())),
            ],
        )
        .expect_err("any failure -> error");
        assert!(err.contains("added 2 of 3"), "{err}");
        assert!(err.contains("1 failed"), "{err}");
        // Per-record breakdown names both the created and the failed values.
        assert!(err.contains("✓ 1.2.3.4"), "{err}");
        assert!(err.contains("✗ 5.6.7.8 — 422 invalid data"), "{err}");
        assert!(err.contains("✓ 9.9.9.9"), "{err}");
        // The recovery hint must include the domain positional or it won't parse.
        assert!(
            err.contains("dns list example.com --type A --name www"),
            "{err}"
        );
    }
}
