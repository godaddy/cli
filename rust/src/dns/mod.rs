//! `gddy dns` — manage a domain's DNS records (A, AAAA, CNAME, MX, TXT, …).
//!
//! These are the record operations of the same GoDaddy Domains API the
//! [`crate::domain`] commands use (paths under `/v1/domains/{domain}/records`),
//! served by the typed, spec-generated [`domains_client`] crate. Auth, the
//! base-URL/environment resolution, and the sso-key/Bearer scheme selection are
//! shared with `domain` via [`crate::domain::make_client`].
//!
//! Reads (`list`) require the `domains.domain:read` scope; mutations (`add`,
//! `set`, `delete`) require `domains.dns:update`. `set` and `delete` are
//! [`Tier::Destructive`] — they overwrite or remove existing records — so
//! `--dry-run` short-circuits them with a preview, and they carry the engine's
//! global `--reason` flag for audit (`add` only appends, so it is `Mutate`).

use cli_engine::{
    CliCoreError, CommandContext, CommandResult, CommandSpec, GroupSpec, Module,
    RuntimeCommandSpec, RuntimeGroupSpec, Tier,
};
use serde_json::{Value, json};

use crate::domain::{DOMAINS_DNS_UPDATE_SCOPE, DOMAINS_READ_SCOPE, make_client, string_list};
use crate::output_schema::output_schema;

use domains_client::types;

// Output shapes for the mutating commands (their handlers emit these confirmation
// objects), registered so `--help`/`--schema` list the fields like the reads do.
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

/// The eight DNS record types the Domains API accepts, for help text and error
/// messages. Source of truth for the wire values is the generated
/// [`types::DnsRecordType`].
const RECORD_TYPES: &str = "A, AAAA, CNAME, MX, NS, SOA, SRV, TXT";

fn arg_str(ctx: &CommandContext, key: &str) -> Option<String> {
    ctx.args
        .get(key)
        .and_then(|v| v.as_str())
        .map(str::to_owned)
}

/// clap value-parser for `--type`: validate against the record-type set and
/// return the canonical upper-case wire string.
///
/// Validating in clap (rather than the handler) means invalid input is rejected
/// at parse time — *before* cli-engine's `--dry-run`/auth short-circuits — so
/// e.g. `dns set … --type BOGUS --dry-run` fails instead of reporting success.
/// The handlers can then trust the value and feed it straight to the builders.
fn parse_type_arg(raw: &str) -> Result<String, String> {
    let upper = raw.to_ascii_uppercase();
    types::DnsRecordType::try_from(upper.as_str())
        .map(|_| upper)
        .map_err(|_| format!("invalid record type {raw:?}; expected one of {RECORD_TYPES}"))
}

/// Like [`parse_type_arg`], but for `dns delete`: rejects the registry-managed
/// NS/SOA types the records API can't delete (`recordDeleteTypeName`'s type set
/// omits them), with a clear reason — at parse time, so
/// `dns delete … --type NS --dry-run` fails too.
fn parse_deletable_type_arg(raw: &str) -> Result<String, String> {
    let upper = raw.to_ascii_uppercase();
    if types::RecordDeleteTypeNameType::try_from(upper.as_str()).is_ok() {
        return Ok(upper);
    }
    // Distinguish "valid type, just not deletable" from "not a type at all".
    if types::DnsRecordType::try_from(upper.as_str()).is_ok() {
        Err(format!(
            "{upper} records can't be deleted (NS and SOA records are managed by GoDaddy); \
             deletable types: A, AAAA, CNAME, MX, SRV, TXT"
        ))
    } else {
        Err(format!(
            "invalid record type {raw:?}; expected one of A, AAAA, CNAME, MX, SRV, TXT"
        ))
    }
}

/// Optional record fields shared by `add` and `set`.
struct RecordOptions {
    ttl: Option<i64>,
    priority: Option<i64>,
    port: Option<std::num::NonZeroU64>,
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
            // The parser bounds --port to 1..=65535, so a non-zero value always
            // fits NonZeroU64; `new` keeps it total without an unwrap.
            port: as_i64("port")
                .and_then(|p| u64::try_from(p).ok())
                .and_then(std::num::NonZeroU64::new),
            weight: as_i64("weight"),
            protocol: arg_str(ctx, "protocol"),
            service: arg_str(ctx, "service"),
        }
    }
}

/// Build full `DnsRecord`s (type + name + data) for `add` — one per `--data`.
fn dns_records(
    name: &str,
    ty: types::DnsRecordType,
    data: &[String],
    opts: &RecordOptions,
) -> Vec<types::DnsRecord> {
    data.iter()
        .map(|d| types::DnsRecord {
            data: d.clone(),
            name: name.to_owned(),
            type_: ty,
            ttl: opts.ttl,
            priority: opts.priority,
            port: opts.port,
            weight: opts.weight,
            protocol: opts.protocol.clone(),
            service: opts.service.clone(),
        })
        .collect()
}

/// Build the type+name-relative record bodies for `set` (the type and name come
/// from the URL path, so they are omitted here) — one per `--data`.
fn create_type_name_records(
    data: &[String],
    opts: &RecordOptions,
) -> Vec<types::DnsRecordCreateTypeName> {
    data.iter()
        .map(|d| types::DnsRecordCreateTypeName {
            data: d.clone(),
            ttl: opts.ttl,
            priority: opts.priority,
            port: opts.port,
            weight: opts.weight,
            protocol: opts.protocol.clone(),
            service: opts.service.clone(),
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
            .help("Time-to-live in seconds"),
    )
    .with_arg(
        clap::Arg::new("priority")
            .long("priority")
            .value_name("N")
            .value_parser(clap::value_parser!(i64).range(0..))
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
            .value_parser(clap::value_parser!(i64).range(0..))
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
        RuntimeGroupSpec::new(GroupSpec::new("dns", "Manage a domain's DNS records"))
            // --- list -------------------------------------------------------
            .with_command(RuntimeCommandSpec::new_with_context(
                CommandSpec::new("list", "List DNS records for a domain")
                    .with_system("domain")
                    .with_tier(Tier::Read)
                    .with_default_fields("type,name,data,ttl")
                    .with_json_schema::<types::DnsRecord>()
                    .with_scopes(&[DOMAINS_READ_SCOPE])
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
                            .help(
                                "Only records of this type (A, AAAA, CNAME, MX, NS, SOA, SRV, TXT)",
                            ),
                    )
                    .with_arg(
                        clap::Arg::new("name")
                            .long("name")
                            .value_name("NAME")
                            // The API only filters by name within a type, so clap
                            // enforces `--name` requires `--type` at parse time.
                            .requires("type")
                            .help("Only records with this name (requires --type)"),
                    ),
                // Output pagination is the engine's job: the global, client-side
                // `--limit`/`--offset` flags slice the returned list. We don't
                // expose the API's server-side offset/limit (it would double-skip
                // against the client-side `--offset`), so `record_get` is called
                // without them.
                |ctx| async move {
                    let domain = arg_str(&ctx, "domain").unwrap_or_default();
                    // `--type` is validated + upper-cased by clap's value parser,
                    // and `--name` requires `--type`, so these can be used as-is.
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

                    // Serialize each record to an object so `--fields` and the
                    // default-field projection have `type`/`name`/`data`/`ttl`.
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
            // --- add --------------------------------------------------------
            .with_command(RuntimeCommandSpec::new_with_context(
                with_record_write_args(
                    CommandSpec::new(
                        "add",
                        "Add DNS records to a domain (appends; non-destructive)",
                    )
                    .with_system("domain")
                    .with_tier(Tier::Mutate)
                    .with_default_fields("domain,type,name,records")
                    .with_output_schema::<DnsWriteResult>()
                    .with_scopes(&[DOMAINS_DNS_UPDATE_SCOPE]),
                ),
                |ctx| async move {
                    let domain = arg_str(&ctx, "domain").unwrap_or_default();
                    // `--type` is validated + upper-cased by clap's value parser.
                    let record_type = arg_str(&ctx, "type").unwrap_or_default();
                    let name = arg_str(&ctx, "name").unwrap_or_default();
                    let data = string_list(&ctx, "data");
                    let ty = types::DnsRecordType::try_from(record_type.as_str())
                        .expect("--type value parser guarantees a valid record type");
                    let opts = RecordOptions::from_ctx(&ctx);
                    let records = dns_records(&name, ty, &data, &opts);
                    let count = records.len();

                    let client = make_client(&ctx).await?;
                    client
                        .record_add()
                        .domain(domain.as_str())
                        .body(records)
                        .send()
                        .await
                        .map_err(|e| {
                            CliCoreError::message(format!("adding DNS records failed: {e}"))
                        })?;

                    Ok(CommandResult::new(json!({
                        "domain": domain,
                        "type": record_type,
                        "name": name,
                        "records": count,
                        "action": "add",
                    })))
                },
            ))
            // --- set --------------------------------------------------------
            .with_command(RuntimeCommandSpec::new_with_context(
                with_record_write_args(
                    CommandSpec::new(
                        "set",
                        "Replace all records for a type+name (destructive: overwrites existing)",
                    )
                    .with_system("domain")
                    .with_tier(Tier::Destructive)
                    .with_default_fields("domain,type,name,records")
                    .with_output_schema::<DnsWriteResult>()
                    .with_scopes(&[DOMAINS_DNS_UPDATE_SCOPE]),
                ),
                |ctx| async move {
                    let domain = arg_str(&ctx, "domain").unwrap_or_default();
                    // `--type` is validated + upper-cased by clap's value parser.
                    let record_type = arg_str(&ctx, "type").unwrap_or_default();
                    let name = arg_str(&ctx, "name").unwrap_or_default();
                    let data = string_list(&ctx, "data");
                    let opts = RecordOptions::from_ctx(&ctx);
                    let records = create_type_name_records(&data, &opts);
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
            // --- delete -----------------------------------------------------
            .with_command(RuntimeCommandSpec::new_with_context(
                CommandSpec::new(
                    "delete",
                    "Delete all records for a type+name (destructive: removes existing)",
                )
                .with_system("domain")
                .with_tier(Tier::Destructive)
                .with_default_fields("domain,type,name,deleted")
                .with_output_schema::<DnsDeleteResult>()
                .with_scopes(&[DOMAINS_DNS_UPDATE_SCOPE])
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
                    // `--type` is validated (and NS/SOA rejected) + upper-cased by
                    // clap's value parser, so it converts to the delete enum cleanly.
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
        // NS/SOA are valid types but GoDaddy-managed — rejected as non-deletable
        // (not the generated client's opaque conversion error).
        for ty in ["NS", "soa"] {
            let err = parse_deletable_type_arg(ty).expect_err("should reject");
            assert!(err.contains("managed by GoDaddy"), "got: {err}");
            assert!(
                !err.contains("RecordDeleteTypeNameType"),
                "leaked type name: {err}"
            );
        }
        // A non-type is rejected as invalid, not as non-deletable.
        let err = parse_deletable_type_arg("bogus").expect_err("should reject");
        assert!(err.contains("invalid record type"), "got: {err}");
    }

    /// Type/flag validation lives in clap value-parsers, so invalid input is
    /// rejected at parse time — before auth or `--dry-run` can short-circuit.
    /// (Regression guard: these previously validated only in the handler, so
    /// `--dry-run` reported success and `--name` without `--type` reached auth.)
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
            // Undeletable type — rejected with the GoDaddy reason, not an auth error.
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
            // Bogus type on a mutating command.
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
            // `--name` without `--type` on list.
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

    #[test]
    fn dns_records_builds_one_per_data_value() {
        let opts = RecordOptions {
            ttl: Some(600),
            priority: None,
            port: None,
            weight: None,
            protocol: None,
            service: None,
        };
        let recs = dns_records(
            "www",
            types::DnsRecordType::A,
            &["1.2.3.4".to_string(), "5.6.7.8".to_string()],
            &opts,
        );
        assert_eq!(recs.len(), 2);
        assert_eq!(recs[0].name, "www");
        assert_eq!(recs[0].type_, types::DnsRecordType::A);
        assert_eq!(recs[0].data, "1.2.3.4");
        assert_eq!(recs[0].ttl, Some(600));
        assert_eq!(recs[1].data, "5.6.7.8");
    }

    #[test]
    fn create_type_name_records_omit_type_and_name() {
        let opts = RecordOptions {
            ttl: None,
            priority: Some(10),
            port: None,
            weight: None,
            protocol: None,
            service: None,
        };
        let recs = create_type_name_records(&["mail.example.com".to_string()], &opts);
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].data, "mail.example.com");
        assert_eq!(recs[0].priority, Some(10));
    }

    /// Like the `domain` commands, the `dns` commands hit the Domains API and
    /// must stay fail-closed: built with no auth provider registered, the
    /// engine's default `AuthRequirement::Required` rejects them at credential
    /// resolution (exit code 2) before any handler runs. Running each leaf also
    /// exercises clap's command-tree construction (duplicate-subcommand panics
    /// only surface in debug builds).
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
