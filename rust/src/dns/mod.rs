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
    CliCoreError, CommandContext, CommandResult, CommandSpec, GroupSpec, Module, Result,
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

/// Validate a user-supplied record type case-insensitively, returning the
/// canonical upper-case wire string (for the builders' `type_` setters) and the
/// parsed [`types::DnsRecordType`] (for record bodies).
fn parse_record_type(raw: &str) -> Result<(String, types::DnsRecordType)> {
    let upper = raw.to_uppercase();
    let ty = types::DnsRecordType::try_from(upper.as_str()).map_err(|_| {
        CliCoreError::message(format!(
            "invalid --type {raw:?}; expected one of {RECORD_TYPES}"
        ))
    })?;
    Ok((upper, ty))
}

/// Map an upper-cased record type to the delete endpoint's type enum.
///
/// `recordDeleteTypeName`'s type set omits NS and SOA — GoDaddy manages those
/// records, so they can't be deleted through the records API. Surface a clear
/// error for them instead of the generated client's opaque "conversion to
/// `RecordDeleteTypeNameType` failed". `upper` is assumed already validated as a
/// real record type by [`parse_record_type`], so NS/SOA is the only way this
/// fails.
fn deletable_record_type(upper: &str) -> Result<types::RecordDeleteTypeNameType> {
    types::RecordDeleteTypeNameType::try_from(upper).map_err(|_| {
        CliCoreError::message(format!(
            "{upper} records can't be deleted (NS and SOA records are managed by GoDaddy); \
             deletable types: A, AAAA, CNAME, MX, SRV, TXT"
        ))
    })
}

/// Why a `list` invocation's flag combination is invalid, or `None` when it is
/// valid. Pure so the rule is unit-testable without a context.
///
/// The API only filters by `name` within a `type` (the record routes are
/// `/records`, `/records/{type}`, `/records/{type}/{name}`), so `--name`
/// requires `--type`.
fn list_flag_error(has_type: bool, has_name: bool) -> Option<&'static str> {
    if has_name && !has_type {
        return Some("--name requires --type");
    }
    None
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
                        clap::Arg::new("type").long("type").value_name("TYPE").help(
                            "Only records of this type (A, AAAA, CNAME, MX, NS, SOA, SRV, TXT)",
                        ),
                    )
                    .with_arg(
                        clap::Arg::new("name")
                            .long("name")
                            .value_name("NAME")
                            .help("Only records with this name (requires --type)"),
                    ),
                // Output pagination is the engine's job: the global, client-side
                // `--limit`/`--offset` flags slice the returned list. We don't
                // expose the API's server-side offset/limit (it would double-skip
                // against the client-side `--offset`), so `record_get` is called
                // without them.
                |ctx| async move {
                    let domain = arg_str(&ctx, "domain").unwrap_or_default();
                    let type_opt = arg_str(&ctx, "type");
                    let name_opt = arg_str(&ctx, "name");

                    if let Some(err) = list_flag_error(type_opt.is_some(), name_opt.is_some()) {
                        return Err(CliCoreError::message(err));
                    }

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
                        (Some(raw_type), None) => {
                            let (upper, _) = parse_record_type(raw_type)?;
                            client
                                .record_get_by_type()
                                .domain(domain.as_str())
                                .type_(upper.as_str())
                                .send()
                                .await
                                .map_err(|e| {
                                    CliCoreError::message(format!(
                                        "listing DNS records failed: {e}"
                                    ))
                                })?
                                .into_inner()
                        }
                        (Some(raw_type), Some(name)) => {
                            let (upper, _) = parse_record_type(raw_type)?;
                            client
                                .record_get()
                                .domain(domain.as_str())
                                .type_(upper.as_str())
                                .name(name)
                                .send()
                                .await
                                .map_err(|e| {
                                    CliCoreError::message(format!(
                                        "listing DNS records failed: {e}"
                                    ))
                                })?
                                .into_inner()
                        }
                    };

                    // Serialize each record to an object so `--fields` and the
                    // default-field projection have `type`/`name`/`data`/`ttl`.
                    let out: Vec<Value> = records
                        .iter()
                        .map(serde_json::to_value)
                        .collect::<std::result::Result<_, _>>()
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
                    let raw_type = arg_str(&ctx, "type").unwrap_or_default();
                    let name = arg_str(&ctx, "name").unwrap_or_default();
                    let data = string_list(&ctx, "data");
                    let (_, ty) = parse_record_type(&raw_type)?;
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
                        "type": raw_type.to_uppercase(),
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
                    let raw_type = arg_str(&ctx, "type").unwrap_or_default();
                    let name = arg_str(&ctx, "name").unwrap_or_default();
                    let data = string_list(&ctx, "data");
                    let (upper, _) = parse_record_type(&raw_type)?;
                    let opts = RecordOptions::from_ctx(&ctx);
                    let records = create_type_name_records(&data, &opts);
                    let count = records.len();

                    let client = make_client(&ctx).await?;
                    client
                        .record_replace_type_name()
                        .domain(domain.as_str())
                        .type_(upper.as_str())
                        .name(name.as_str())
                        .body(records)
                        .send()
                        .await
                        .map_err(|e| {
                            CliCoreError::message(format!("replacing DNS records failed: {e}"))
                        })?;

                    Ok(CommandResult::new(json!({
                        "domain": domain,
                        "type": upper,
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
                        .help("Record type (one of A, AAAA, CNAME, MX, NS, SOA, SRV, TXT)"),
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
                    let raw_type = arg_str(&ctx, "type").unwrap_or_default();
                    let name = arg_str(&ctx, "name").unwrap_or_default();
                    let (upper, _) = parse_record_type(&raw_type)?;
                    let delete_type = deletable_record_type(&upper)?;

                    let client = make_client(&ctx).await?;
                    client
                        .record_delete_type_name()
                        .domain(domain.as_str())
                        .type_(delete_type)
                        .name(name.as_str())
                        .send()
                        .await
                        .map_err(|e| {
                            CliCoreError::message(format!("deleting DNS records failed: {e}"))
                        })?;

                    Ok(CommandResult::new(json!({
                        "domain": domain,
                        "type": upper,
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
    fn parse_record_type_is_case_insensitive_and_canonicalizes() {
        let (upper, ty) = parse_record_type("aaaa").expect("valid");
        assert_eq!(upper, "AAAA");
        assert_eq!(ty, types::DnsRecordType::Aaaa);
        let (upper, ty) = parse_record_type("Cname").expect("valid");
        assert_eq!(upper, "CNAME");
        assert_eq!(ty, types::DnsRecordType::Cname);
    }

    #[test]
    fn deletable_record_type_rejects_ns_and_soa_with_clear_message() {
        // Deletable types convert successfully.
        assert!(deletable_record_type("A").is_ok());
        assert!(deletable_record_type("CNAME").is_ok());
        assert!(deletable_record_type("TXT").is_ok());
        // NS/SOA are managed by GoDaddy — rejected with an actionable message
        // (not the generated client's opaque conversion error).
        for ty in ["NS", "SOA"] {
            let err = deletable_record_type(ty).expect_err("should reject");
            let msg = err.to_string();
            assert!(msg.contains("managed by GoDaddy"), "got: {msg}");
            assert!(
                !msg.contains("RecordDeleteTypeNameType"),
                "leaked type name: {msg}"
            );
        }
    }

    #[test]
    fn parse_record_type_rejects_unknown() {
        let err = parse_record_type("bogus").expect_err("should reject");
        let msg = err.to_string();
        assert!(msg.contains("invalid --type"), "got: {msg}");
        assert!(msg.contains("AAAA"), "lists valid types, got: {msg}");
    }

    #[test]
    fn list_flag_error_requires_type_for_name() {
        // Valid combinations.
        assert_eq!(list_flag_error(false, false), None); // list all
        assert_eq!(list_flag_error(true, false), None); // by type
        assert_eq!(list_flag_error(true, true), None); // by type + name
        // --name without --type is rejected.
        assert_eq!(list_flag_error(false, true), Some("--name requires --type"));
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
