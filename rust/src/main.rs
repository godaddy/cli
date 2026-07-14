mod actions_catalog;
mod api_explorer;
mod application;
mod auth;
mod config;
mod contacts;
mod dns;
mod domain;
mod env;
mod environments;
mod extension;
mod hosting;
mod next_action;
mod output_schema;
mod pat;
mod payments;
mod quote_cache;
mod scopes;
mod update;
mod webhook;

use std::{process::ExitCode, sync::Arc};

use clap::{Arg, ArgAction};
use cli_engine::{BuildInfo, Cli, CliConfig};

use crate::env::get_env;
use crate::next_action::next_action;

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    let auth_provider = Arc::new(auth::GoDaddyAuthProvider::new());

    let cli = Cli::new(
        CliConfig::new("gddy", "GoDaddy developer CLI", "gddy")
            .with_long(
                "gddy is the command-line interface to the GoDaddy developer platform.\n\
                 \n\
                 What you can do:\n  \
                 • domain   — list your domains, check availability, get suggestions, and register new ones\n  \
                 • dns      — view and edit a domain's DNS records\n  \
                 • api      — explore and call GoDaddy REST API endpoints directly\n  \
                 • application — build, configure, and deploy platform applications\n  \
                 • hosting  — manage Node.js PaaS applications (create, upload, deploy)\n  \
                 • payments — manage the payment methods used for purchases\n\
                 \n\
                 Most commands need authentication; run `gddy auth login` first, or use a PAT via `gddy pat add` / `GDDY_PAT` for non-interactive workflows (or just run a\n\
                 command and follow the prompt). Use `--env` to target an environment and\n\
                 `gddy tree` to see every command. New here? Try `gddy domain available <name>`.",
            )
            .with_build(
                BuildInfo::new(env!("CARGO_PKG_VERSION"))
                    .with_commit(env!("GDDY_BUILD_COMMIT"))
                    .with_date(env!("GDDY_BUILD_DATE")),
            )
            .with_default_auth_provider("godaddy")
            .with_auth_provider(auth_provider)
            .with_register_flags(Arc::new(|cmd| {
                cmd.disable_help_flag(true)
                    .arg(
                        // clap's default help arg shows a short summary for
                        // `-h` and the full text for `--help`. Override it so
                        // both flags print the same full help everywhere.
                        Arg::new("help")
                            .short('h')
                            .long("help")
                            .action(ArgAction::HelpLong)
                            .global(true)
                            .help("Print help"),
                    )
                    .arg(
                        Arg::new("env")
                            .long("env")
                            .global(true)
                            .value_name("ENV")
                            .default_value(
                                get_env().unwrap_or_else(|| environments::DEFAULT_ENV.to_owned()),
                            )
                            .help("Target environment: prod or ote"),
                    )
            }))
            .with_apply_flags(Arc::new(|matches, mw| {
                if let Some(env) = matches.get_one::<String>("env") {
                    // Validate only an explicitly-provided `--env`. The default
                    // value comes from `.gdenv`/DEFAULT_ENV (already filtered), so
                    // validating it unconditionally would let a malformed local
                    // config fail *every* command — even ones not using `--env`.
                    if matches.value_source("env") == Some(clap::parser::ValueSource::CommandLine) {
                        environments::resolve(env)
                            .map_err(|e| cli_engine::CliCoreError::message(e.to_string()))?;
                    }
                    mw.env = env.clone();
                }
                Ok(())
            }))
            .with_root_next_actions(Arc::new(|| {
                vec![
                    next_action("auth status", "Check authentication status"),
                    next_action("env get", "Get the current active environment"),
                    next_action("application list", "List all applications"),
                    next_action("tree", "Display the full command tree"),
                ]
            }))
            .with_pre_run(Arc::new(|_mw, _path, _args| {
                update::maybe_spawn_background_refresh();
                Ok(())
            }))
            .with_on_shutdown(Arc::new(update::maybe_print_update_notice))
            .with_module(actions_catalog::module())
            .with_module(api_explorer::module())
            .with_module(application::module())
            .with_module(dns::module())
            .with_module(domain::module())
            .with_module(env::module())
            .with_module(hosting::module())
            .with_module(pat::module())
            .with_module(payments::module())
            .with_module(update::module())
            .with_module(webhook::module()),
    );

    cli.execute().await
}
