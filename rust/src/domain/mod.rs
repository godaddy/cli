//! `gddy domain` — domain discovery, registration, and account domains.
//!
//! These commands span two generations of the GoDaddy Domains API behind one
//! host (see the [`domains_client`] crate):
//!
//! * **v3** (Domain Lifecycle Management API) — `available`, `suggest`, `get`,
//!   `quote`, `purchase` (quote → register), and `nameservers set`.
//! * **v1** — `list` (the shopper's domains) and `agreements` (a TLD's legal
//!   agreements), which v3 does not yet serve.
//!
//! All of these authenticate with an OAuth bearer token from
//! [`GoDaddyAuthProvider`](crate::auth::GoDaddyAuthProvider). The v3 registration
//! flow (`purchase`) requires an OAuth customer token (it mints a single-use
//! quote token, records consent, and registers under the customer).
//!
//! Each subcommand lives in its own module; [`common`] holds the shared
//! authenticated client, money formatting, and API-error rendering.

use cli_engine::{GroupSpec, Module, RuntimeGroupSpec};

mod agreements;
mod available;
mod common;
mod contacts;
mod get;
mod list;
mod nameservers;
mod operation;
mod purchase;
mod quote;
mod suggest;

// Shared with the `dns` module, which builds the same Domains API client and
// reuses the repeatable-argument helper.
pub(crate) use common::{api_error, format_api_error, make_client};

pub fn module() -> Module {
    Module::new("Domains", |_ctx| {
        RuntimeGroupSpec::new(
            GroupSpec::new(
                "domain",
                "List your domains, check availability, and register new ones",
            )
            .with_long(
                "Work with domains on your GoDaddy account and the public registry.\n\
             \n\
             • list / get        — your existing domains and their details\n\
             • available / suggest — find a name to register\n\
             • quote             — price a registration and see required agreements\n\
             • purchase          — register a new domain (charges your account)\n\
             • nameservers set   — point a domain at custom nameservers\n\
             • operation status  — check on an async operation (e.g. a pending purchase)\n\
             \n\
             Reads need the `domains.domain:read` scope; purchase also needs\n\
             `domains.domain:create`, and `nameservers set` needs\n\
             `domains.nameserver:update`. Manage a domain's DNS with `gddy dns`.",
            ),
        )
        .with_command(list::command())
        .with_command(get::command())
        .with_command(available::command())
        .with_command(suggest::command())
        .with_command(agreements::command())
        .with_command(quote::command())
        .with_command(purchase::command())
        .with_group(nameservers::group())
        .with_group(contacts::group())
        .with_group(operation::group())
    })
    .with_guides_from_markdown([(
        "domain-purchase.md",
        include_bytes!("guides/domain-purchase.md").as_slice(),
    )])
}

#[cfg(test)]
mod tests {
    use cli_engine::{Cli, CliConfig};

    /// The `domain` commands call the Domains API, so they must stay fail-closed.
    /// Built with no auth provider registered, the engine's default
    /// `AuthRequirement::Required` must reject them at credential resolution
    /// (exit code 2) before the handler runs. Running each leaf also exercises
    /// clap's command-tree construction (duplicate-subcommand panics surface only
    /// in debug builds).
    #[tokio::test]
    async fn domain_commands_require_auth() {
        const AUTH_FAILURE_EXIT: i32 = 2;
        let cases: [&[&str]; 9] = [
            &["gddy", "domain", "list", "--output", "json"],
            &["gddy", "domain", "get", "example.com", "--output", "json"],
            &[
                "gddy",
                "domain",
                "available",
                "example.com",
                "--output",
                "json",
            ],
            &["gddy", "domain", "suggest", "coffee", "--output", "json"],
            &[
                "gddy",
                "domain",
                "agreements",
                "--tld",
                "com",
                "--output",
                "json",
            ],
            &["gddy", "domain", "quote", "example.com", "--output", "json"],
            // `--quote-token` is required, so pass a dummy one — otherwise clap's
            // missing-arg error (exit 2) would mask the auth-provider assertion.
            &[
                "gddy",
                "domain",
                "purchase",
                "--quote-token",
                "dummy-token",
                "--agree",
                "--confirm",
                "--output",
                "json",
            ],
            &[
                "gddy",
                "domain",
                "nameservers",
                "set",
                "example.com",
                "--nameserver",
                "ns1.example.net",
                "--output",
                "json",
            ],
            &[
                "gddy",
                "domain",
                "operation",
                "status",
                "dummy-op-id",
                "--output",
                "json",
            ],
        ];
        for args in cases {
            let cli = Cli::new(
                CliConfig::new("gddy", "GoDaddy developer CLI", "gddy")
                    .with_default_auth_provider("godaddy")
                    .with_module(super::module()),
            );
            let output = cli.run(args.iter().copied()).await;
            assert_eq!(
                output.exit_code, AUTH_FAILURE_EXIT,
                "{args:?} must fail closed at auth resolution, got: {}",
                output.rendered
            );
            let json: serde_json::Value =
                serde_json::from_str(&output.rendered).expect("valid json output");
            let message = json["error"]["message"].as_str().unwrap_or_default();
            assert!(
                message.contains("provider"),
                "expected an auth-provider resolution error for {args:?}, got: {}",
                output.rendered
            );
        }
    }
}
