mod actions_catalog;
mod api_explorer;
mod application;
mod auth;
mod config;
mod env;
mod extension;
mod webhook;

use std::{process::ExitCode, sync::Arc};

use clap::Arg;
use cli_engine::{BuildInfo, Cli, CliConfig};

use crate::env::get_env;

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    let auth_provider = Arc::new(auth::GoDaddyAuthProvider::new());

    let cli = Cli::new(
        CliConfig::new("godaddy", "GoDaddy developer CLI", "godaddy")
            .with_build(BuildInfo::new(env!("CARGO_PKG_VERSION")))
            .with_default_auth_provider("godaddy")
            .with_auth_provider(auth_provider)
            .with_register_flags(Arc::new(|cmd| {
                cmd.arg(
                    Arg::new("env")
                        .long("env")
                        .global(true)
                        .value_name("ENV")
                        .default_value(get_env().unwrap_or_else(|| "ote".to_owned()))
                        .help("Target environment (ote|prod)"),
                )
            }))
            .with_apply_flags(Arc::new(|matches, mw| {
                if let Some(env) = matches.get_one::<String>("env") {
                    mw.env = env.clone();
                }
                Ok(())
            }))
            .with_module(actions_catalog::module())
            .with_module(api_explorer::module())
            .with_module(application::module())
            .with_module(env::module())
            .with_module(webhook::module()),
    );

    cli.execute().await
}
