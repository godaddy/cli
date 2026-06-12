mod actions_catalog;
mod api_explorer;
mod application;
mod auth;
mod config;
mod domain;
mod env;
mod environments;
mod extension;
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

    let auth_provider = Arc::new(auth::CompositeAuthProvider::new());

    let cli = Cli::new(
        CliConfig::new("gddy", "GoDaddy developer CLI", "gddy")
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
                        .help("Target environment (ote|prod)"),
                )
                .arg(
                    Arg::new("api-key")
                        .long("api-key")
                        .global(true)
                        .value_name("KEY")
                        .requires("api-secret")
                        .help("sso-key API key for domain commands (overrides config/env)"),
                )
                .arg(
                    Arg::new("api-secret")
                        .long("api-secret")
                        .global(true)
                        .value_name("SECRET")
                        .requires("api-key")
                        .help("sso-key API secret (used with --api-key)"),
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
                // Bridge the sso-key flags to the auth layer (the composite
                // provider reads this for `domain:*` commands). clap enforces that
                // the two flags are supplied together (`requires`), so checking
                // both here is just defensive.
                if let (Some(key), Some(secret)) = (
                    matches.get_one::<String>("api-key"),
                    matches.get_one::<String>("api-secret"),
                ) {
                    auth::set_api_key_override(key.clone(), secret.clone());
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
            .with_module(domain::module())
            .with_module(env::module())
            .with_module(webhook::module()),
    );

    cli.execute().await
}
