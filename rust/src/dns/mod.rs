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

use cli_engine::{GroupSpec, Module, RuntimeGroupSpec};

mod add;
mod conflicts;
mod delete;
mod list;
mod records;
mod set;

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
        .with_command(list::command())
        .with_command(add::command())
        .with_command(set::command())
        .with_command(delete::command())
    })
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
}
