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
//! * **`export`** — `GET` every record and write it as a zone file (RFC 1035, via
//!   the `rfc1035-format` crate) or as `list`'s JSON; see [`zone_records`].
//! * **`import`** — read a zone file or `list`'s JSON, check every record, and
//!   make the changes with `set`'s per-record writes. By default it only adds;
//!   `--replace` also deletes what the input lacks. See [`import_plan`].
//!
//! Reads (`list`, `export`) require the `domains.domain:read` scope; mutations
//! (`add`, `set`, `delete`, `import`) require `domains.dns:update`. `set`,
//! `delete` and `import` are [`Tier::Destructive`] — they overwrite or remove
//! existing records (`import` only with `--replace`, but a command has one tier)
//! — so `--dry-run` short-circuits them with a preview.

use cli_engine::{CliCoreError, GroupSpec, Module, RuntimeGroupSpec};

mod add;
mod conflicts;
mod delete;
mod export;
mod failure;
mod import;
mod import_plan;
mod list;
pub(crate) mod records;
mod set;
mod zone_records;

/// A failed add/set/delete: reported with the code of its failures' shared kind
/// (see [`failure`]), or as unexpected when they differ.
fn mutate_failed(failure: failure::MutateFailure) -> CliCoreError {
    failure.into_cli_error()
}

pub fn module() -> Module {
    Module::new("DNS", |_ctx| {
        RuntimeGroupSpec::new(
            GroupSpec::new("dns", "Manage a domain's DNS records").with_long(
                "Create, read, and delete DNS records for a GoDaddy-managed \
                domain. `add` appends new records without touching existing \
                ones; `set` replaces every record for a given type+name pair; \
                `delete` removes all records for a type+name pair. `export` \
                writes all records as a zone file, and `import` adds records \
                from one. Pass `--dry-run` to preview changes from commands.",
            ),
        )
        .with_command(list::command())
        .with_command(add::command())
        .with_command(set::command())
        .with_command(delete::command())
        .with_command(export::command())
        .with_command(import::command())
    })
    .with_guides_from_markdown([("dns.md", include_bytes!("guides/dns.md").as_slice())])
}

#[cfg(test)]
mod tests {
    use super::*;
    use cli_engine::{Cli, CliConfig};

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
        let cases: [(&[&str], &str); 6] = [
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
            (
                &[
                    "gddy",
                    "dns",
                    "add",
                    "example.com",
                    "--type",
                    "TLSA",
                    "--name",
                    "www",
                    "--usage",
                    "3",
                    "--selector",
                    "1",
                    "--matching-type",
                    "1",
                ],
                "--data",
            ),
            // A lower-case --type must require the same fields as an upper-case
            // one. clap compares `required_if_eq` against the raw argument, so
            // these two only hold while `--type` sets `ignore_case`.
            (
                &[
                    "gddy",
                    "dns",
                    "add",
                    "example.com",
                    "--type",
                    "tlsa",
                    "--name",
                    "www",
                    "--data",
                    "d2abde240d7cd3ee6b4b28c54df034b97983a1d16e8a410e4561cb106618e971",
                ],
                "--usage",
            ),
            (
                &[
                    "gddy",
                    "dns",
                    "set",
                    "example.com",
                    "--type",
                    "caa",
                    "--name",
                    "@",
                    "--data",
                    "letsencrypt.org",
                ],
                "--tag",
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

    /// `add`/`set`/`delete` opted into handler-driven `--dry-run` (to build a real
    /// preview via `fetch_records`), which means the handler now runs under
    /// `--dry-run` too — unlike the generic short-circuit, that must not skip
    /// the fail-closed `Required`-auth check.
    #[tokio::test]
    async fn dns_write_dry_runs_still_require_auth() {
        const AUTH_FAILURE_EXIT: i32 = 2;
        let cases: [&[&str]; 3] = [
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
                "5.6.7.8",
                "--dry-run",
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
                "--dry-run",
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
                "--dry-run",
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
                "{args:?} must still fail closed at auth resolution under --dry-run, got: {}",
                output.rendered
            );
        }
    }
}
