//! `gddy dns` — manage a domain's DNS records (A, AAAA, CNAME, MX, TXT, …).
//!
//! These records span two API generations of the Domains API (see the
//! [`domains_client`] crate), sharing auth/env resolution with `domain` via
//! [`crate::domain::make_client`]:
//!
//! * **`add`** creates a single record via the v3 endpoint
//!   (`POST /v3/domains/zones/{zone}/dns-records`); one API call per `--data`
//!   value, and `ttl` is required (defaults to 3600 when omitted).
//! * **`list` / `set` / `delete`** use the retained v1 record endpoints
//!   (`/v1/domains/{domain}/records…`), which v3 does not yet serve.
//!
//! Reads (`list`) require the `domains.domain:read` scope; mutations (`add`,
//! `set`, `delete`) require `domains.dns:update`. `set` and `delete` are
//! [`Tier::Destructive`] — they overwrite or remove existing records — so
//! `--dry-run` short-circuits them with a preview.

use cli_engine::{
    CliCoreError, CommandContext, CommandResult, CommandSpec, GroupSpec, Module,
    RuntimeCommandSpec, RuntimeGroupSpec, Tier,
};
use serde_json::{Value, json};

use crate::domain::{make_client, string_list};
use crate::output_schema::output_schema;
use crate::scopes::{DOMAINS_DNS_UPDATE, DOMAINS_READ};

use domains_client::types;

output_schema!(DnsWriteResult {
    "domain": "string";
    "type": "string";
    "name": "string";
    "records": "number";
    "action": "string";
});

output_schema!(DnsDeleteResult {
    "domain": "string";
    "type": "string";
    "name": "string";
    "deleted": "bool";
});

/// The DNS record types the Domains API accepts.
const RECORD_TYPES: &[&str] = &["A", "AAAA", "CNAME", "MX", "NS", "SOA", "SRV", "TXT"];
/// Record types `dns delete` can remove — NS/SOA are GoDaddy-managed.
const DELETABLE_TYPES: &[&str] = &["A", "AAAA", "CNAME", "MX", "SRV", "TXT"];
/// Default TTL (seconds) for `dns add` when `--ttl` is omitted (v3 requires a ttl).
const DEFAULT_TTL: i64 = 3600;

fn arg_str(ctx: &CommandContext, key: &str) -> Option<String> {
    ctx.args
        .get(key)
        .and_then(|v| v.as_str())
        .map(str::to_owned)
}

/// clap value-parser for `--type`: validate against the record-type set and
/// return the canonical upper-case wire string. Validating in clap (rather than
/// the handler) rejects invalid input at parse time — before `--dry-run`/auth
/// short-circuits.
fn parse_type_arg(raw: &str) -> Result<String, String> {
    let upper = raw.to_ascii_uppercase();
    if RECORD_TYPES.contains(&upper.as_str()) {
        Ok(upper)
    } else {
        Err(format!(
            "invalid record type {raw:?}; expected one of {}",
            RECORD_TYPES.join(", ")
        ))
    }
}

/// Like [`parse_type_arg`], but for `dns delete`: rejects the registry-managed
/// NS/SOA types with a clear reason (at parse time).
fn parse_deletable_type_arg(raw: &str) -> Result<String, String> {
    let upper = raw.to_ascii_uppercase();
    if DELETABLE_TYPES.contains(&upper.as_str()) {
        return Ok(upper);
    }
    if RECORD_TYPES.contains(&upper.as_str()) {
        Err(format!(
            "{upper} records can't be deleted (NS and SOA records are managed by GoDaddy); \
             deletable types: {}",
            DELETABLE_TYPES.join(", ")
        ))
    } else {
        Err(format!(
            "invalid record type {raw:?}; expected one of {}",
            DELETABLE_TYPES.join(", ")
        ))
    }
}

/// Optional record fields shared by `add` and `set`.
struct RecordOptions {
    ttl: Option<i64>,
    priority: Option<i64>,
    port: Option<i64>,
    weight: Option<i64>,
    protocol: Option<String>,
    service: Option<String>,
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
        }
    }
}

/// Build a v3 `DnsRecord` (for `add`) — one per `--data` value. `ttl` is required
/// by v3, so an omitted `--ttl` uses [`DEFAULT_TTL`]. The numeric SRV/MX fields are
/// clamped into the v3 `u16` domain (the clap parsers already bound them).
fn v3_records(
    name: &str,
    ty: &str,
    data: &[String],
    opts: &RecordOptions,
) -> Vec<types::DnsRecord> {
    let to_u16 = |v: Option<i64>| v.and_then(|n| u16::try_from(n).ok());
    data.iter()
        .map(|d| types::DnsRecord {
            data: d.clone(),
            flag: None,
            name: name.to_owned(),
            port: to_u16(opts.port),
            priority: to_u16(opts.priority),
            protocol: opts.protocol.clone(),
            record_id: None,
            service: opts.service.clone(),
            tag: None,
            ttl: opts.ttl.unwrap_or(DEFAULT_TTL),
            type_: types::DnsRecordType(ty.to_owned()),
            weight: to_u16(opts.weight),
        })
        .collect()
}

/// Build the v1 type+name-relative record bodies for `set` (the type and name
/// come from the URL path) — one per `--data`.
fn v1_set_records(data: &[String], opts: &RecordOptions) -> Vec<types::V1dnsRecordCreateTypeName> {
    data.iter()
        .map(|d| types::V1dnsRecordCreateTypeName {
            data: d.clone(),
            port: opts
                .port
                .and_then(|p| u64::try_from(p).ok())
                .and_then(std::num::NonZeroU64::new),
            priority: opts.priority,
            protocol: opts.protocol.clone(),
            service: opts.service.clone(),
            ttl: opts.ttl,
            weight: opts.weight,
        })
        .collect()
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
            .value_parser(parse_type_arg)
            .help("Record type (one of A, AAAA, CNAME, MX, NS, SOA, SRV, TXT)"),
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
            .help("Time-to-live in seconds (add: defaults to 3600)"),
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
        // --- list (v1) --------------------------------------------------
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
                .with_json_schema::<types::V1dnsRecord>()
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
                        .value_parser(parse_type_arg)
                        .help("Only records of this type (A, AAAA, CNAME, MX, NS, SOA, SRV, TXT)"),
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

                let client = make_client(&ctx).await?;
                let records = match (type_opt.as_deref(), name_opt.as_deref()) {
                    (None, _) => client
                        .record_get_all()
                        .domain(domain.as_str())
                        .send()
                        .await
                        .map_err(|e| {
                            CliCoreError::message(format!("listing DNS records failed: {e}"))
                        })?
                        .into_inner(),
                    (Some(record_type), None) => client
                        .record_get_by_type()
                        .domain(domain.as_str())
                        .type_(record_type)
                        .send()
                        .await
                        .map_err(|e| {
                            CliCoreError::message(format!("listing DNS records failed: {e}"))
                        })?
                        .into_inner(),
                    (Some(record_type), Some(name)) => client
                        .record_get()
                        .domain(domain.as_str())
                        .type_(record_type)
                        .name(name)
                        .send()
                        .await
                        .map_err(|e| {
                            CliCoreError::message(format!("listing DNS records failed: {e}"))
                        })?
                        .into_inner(),
                };

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
                .with_default_fields("domain,type,name,records")
                .with_output_schema::<DnsWriteResult>()
                .with_scopes(&[DOMAINS_DNS_UPDATE]),
            ),
            |ctx| async move {
                let domain = arg_str(&ctx, "domain").unwrap_or_default();
                let record_type = arg_str(&ctx, "type").unwrap_or_default();
                let name = arg_str(&ctx, "name").unwrap_or_default();
                let data = string_list(&ctx, "data");
                let opts = RecordOptions::from_ctx(&ctx);
                let records = v3_records(&name, &record_type, &data, &opts);
                let count = records.len();

                let client = make_client(&ctx).await?;
                // v3 creates a single record per call; add each in turn.
                for record in records {
                    client
                        .create_dns_record()
                        .zone(domain.as_str())
                        .body(record)
                        .send()
                        .await
                        .map_err(|e| {
                            CliCoreError::message(format!("adding DNS record failed: {e}"))
                        })?;
                }

                Ok(CommandResult::new(json!({
                    "domain": domain,
                    "type": record_type,
                    "name": name,
                    "records": count,
                    "action": "add",
                })))
            },
        ))
        // --- set (v1) ---------------------------------------------------
        .with_command(RuntimeCommandSpec::new_with_context(
            with_record_write_args(
                CommandSpec::new(
                    "set",
                    "Replace all records for a type+name (destructive: overwrites existing)",
                )
                .with_long(
                    "Replaces every DNS record for the given type+name pair with the \
                     values supplied via `--data`, discarding any records that were \
                     there before. This operation is destructive and irreversible — \
                     use `dns list --type <type> --name <name>` to review the current \
                     state first. Use `dns add` to append records without removing \
                     existing ones.",
                )
                .with_system("domain")
                .with_tier(Tier::Destructive)
                .with_default_fields("domain,type,name,records")
                .with_output_schema::<DnsWriteResult>()
                .with_scopes(&[DOMAINS_DNS_UPDATE]),
            ),
            |ctx| async move {
                let domain = arg_str(&ctx, "domain").unwrap_or_default();
                let record_type = arg_str(&ctx, "type").unwrap_or_default();
                let name = arg_str(&ctx, "name").unwrap_or_default();
                let data = string_list(&ctx, "data");
                let opts = RecordOptions::from_ctx(&ctx);
                let records = v1_set_records(&data, &opts);
                let count = records.len();

                let client = make_client(&ctx).await?;
                client
                    .record_replace_type_name()
                    .domain(domain.as_str())
                    .type_(record_type.as_str())
                    .name(name.as_str())
                    .body(records)
                    .send()
                    .await
                    .map_err(|e| {
                        CliCoreError::message(format!("replacing DNS records failed: {e}"))
                    })?;

                Ok(CommandResult::new(json!({
                    "domain": domain,
                    "type": record_type,
                    "name": name,
                    "records": count,
                    "action": "replace",
                })))
            },
        ))
        // --- delete (v1) ------------------------------------------------
        .with_command(RuntimeCommandSpec::new_with_context(
            CommandSpec::new(
                "delete",
                "Delete all records for a type+name (destructive: removes existing)",
            )
            .with_long(
                "Removes every DNS record matching the given type+name pair. This \
                 operation is destructive and irreversible — all matching records are \
                 deleted in one call. NS and SOA records are GoDaddy-managed and \
                 cannot be deleted. Use `dns list` to confirm what will be removed \
                 before running this command.",
            )
            .with_system("domain")
            .with_tier(Tier::Destructive)
            .with_default_fields("domain,type,name,deleted")
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
                    .value_parser(parse_deletable_type_arg)
                    .help("Record type (one of A, AAAA, CNAME, MX, SRV, TXT)"),
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

                let client = make_client(&ctx).await?;
                client
                    .record_delete_type_name()
                    .domain(domain.as_str())
                    .type_(record_type.as_str())
                    .name(name.as_str())
                    .send()
                    .await
                    .map_err(|e| {
                        CliCoreError::message(format!("deleting DNS records failed: {e}"))
                    })?;

                Ok(CommandResult::new(json!({
                    "domain": domain,
                    "type": record_type,
                    "name": name,
                    "deleted": true,
                })))
            },
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cli_engine::{Cli, CliConfig};

    #[test]
    fn parse_type_arg_validates_and_uppercases() {
        assert_eq!(parse_type_arg("aaaa").expect("valid"), "AAAA");
        assert_eq!(parse_type_arg("Cname").expect("valid"), "CNAME");
        let err = parse_type_arg("bogus").expect_err("should reject");
        assert!(err.contains("invalid record type"), "got: {err}");
        assert!(err.contains("AAAA"), "lists valid types: {err}");
    }

    #[test]
    fn parse_deletable_type_arg_rejects_ns_and_soa_with_clear_message() {
        assert_eq!(parse_deletable_type_arg("a").expect("valid"), "A");
        assert!(parse_deletable_type_arg("TXT").is_ok());
        for ty in ["NS", "soa"] {
            let err = parse_deletable_type_arg(ty).expect_err("should reject");
            assert!(err.contains("managed by GoDaddy"), "got: {err}");
        }
        let err = parse_deletable_type_arg("bogus").expect_err("should reject");
        assert!(err.contains("invalid record type"), "got: {err}");
    }

    #[test]
    fn v3_records_builds_one_per_data_value_with_default_ttl() {
        let opts = RecordOptions {
            ttl: None,
            priority: None,
            port: None,
            weight: None,
            protocol: None,
            service: None,
        };
        let recs = v3_records(
            "www",
            "A",
            &["1.2.3.4".to_string(), "5.6.7.8".to_string()],
            &opts,
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
    fn v3_records_clamps_numeric_fields_to_u16() {
        let opts = RecordOptions {
            ttl: Some(600),
            priority: Some(10),
            port: Some(443),
            weight: Some(5),
            protocol: Some("tcp".to_string()),
            service: Some("sip".to_string()),
        };
        let recs = v3_records("_sip", "SRV", &["sip.example.com".to_string()], &opts);
        assert_eq!(recs[0].ttl, 600);
        assert_eq!(recs[0].priority, Some(10));
        assert_eq!(recs[0].port, Some(443));
        assert_eq!(recs[0].weight, Some(5));
    }

    #[test]
    fn v1_set_records_omit_type_and_name() {
        let opts = RecordOptions {
            ttl: None,
            priority: Some(10),
            port: None,
            weight: None,
            protocol: None,
            service: None,
        };
        let recs = v1_set_records(&["mail.example.com".to_string()], &opts);
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].data, "mail.example.com");
        assert_eq!(recs[0].priority, Some(10));
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
                "--reason",
                "test",
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
                "--reason",
                "test",
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
}
