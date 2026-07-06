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
mod output_schema;
mod payments;
mod quote_cache;
mod scopes;
mod webhook;

use std::{process::ExitCode, sync::Arc};

use clap::Arg;
use cli_engine::{BuildInfo, Cli, CliConfig, NextAction};

use crate::env::get_env;

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
                 Most commands need authentication; run `gddy auth login` first (or just run a\n\
                 command and follow the prompt). Use `--env` to target an environment and\n\
                 `gddy tree` to see every command. New here? Try `gddy domain available <name>`.",
            )
            .with_build(BuildInfo::new(env!("CARGO_PKG_VERSION")))
            .with_default_auth_provider("godaddy")
            .with_auth_provider(auth_provider)
            .with_register_flags(Arc::new(|cmd| {
                cmd.arg(
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
                    NextAction::new("auth status", "Check authentication status"),
                    NextAction::new("env get", "Get the current active environment"),
                    NextAction::new("application list", "List all applications"),
                    NextAction::new("tree", "Display the full command tree"),
                ]
            }))
            .with_module(actions_catalog::module())
            .with_module(api_explorer::module())
            .with_module(application::module())
            .with_module(dns::module())
            .with_module(domain::module())
            .with_module(env::module())
            .with_module(hosting::module())
            .with_module(payments::module())
            .with_module(webhook::module()),
    );

    cli.execute().await
}
