use cli_engine::{
    CommandResult, CommandSpec, GroupSpec, Module, RuntimeCommandSpec, RuntimeGroupSpec, Tier,
};
use serde_json::json;

use crate::environments::{self, EnvError};

/// Resolve the path to the `.gdenv` state file in the user's home directory.
///
/// Uses `dirs::home_dir()` for cross-platform resolution (`$HOME` on Unix,
/// `%USERPROFILE%`/known folders on Windows) rather than reading `$HOME`
/// directly, which is unset on Windows.
fn gdenv_path() -> std::io::Result<std::path::PathBuf> {
    dirs::home_dir()
        .map(|home| home.join(".gdenv"))
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "could not determine home directory",
            )
        })
}

pub fn get_env() -> Option<String> {
    gdenv_path()
        .ok()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .map(|s| s.trim().to_owned())
        // Ignore empty or unrecognized values (e.g. a hand-edited/corrupted
        // .gdenv) so callers fall back to DEFAULT_ENV instead of propagating an
        // unknown environment. `is_known` accepts built-ins plus any env defined
        // via env var or local config, so a persisted custom env (e.g. "dev")
        // survives.
        .filter(|s| !s.is_empty() && environments::is_known(s))
}

pub fn set_env(env: &str) -> std::io::Result<()> {
    let path = gdenv_path()?;
    std::fs::write(&path, env).map_err(|e| {
        // Include the resolved (platform-dependent) path so callers can surface
        // an actionable message — the raw write error omits it.
        std::io::Error::new(e.kind(), format!("{}: {e}", path.display()))
    })
}

fn map_err(e: EnvError) -> cli_engine::CliCoreError {
    cli_engine::CliCoreError::message(e.to_string())
}

fn active_env() -> String {
    get_env().unwrap_or_else(|| environments::DEFAULT_ENV.to_owned())
}

pub fn module() -> Module {
    Module::new("Admin", |_ctx| {
        RuntimeGroupSpec::new(GroupSpec::new("env", "Manage active GoDaddy environment"))
            .with_command(RuntimeCommandSpec::new(
                CommandSpec::new("list", "List available environments")
                    .with_system("env")
                    .with_tier(Tier::Read)
                    .no_auth(true),
                |_cred, _args| async move {
                    let current = active_env();
                    let mut resolved = environments::listable().map_err(map_err)?;
                    // If the active env is defined only via `<ENV>_API_URL` (so it's
                    // excluded from `listable`), still show it — otherwise the list
                    // has no `active: true` entry and callers can't tell what's active.
                    if !resolved.iter().any(|e| e.name == current)
                        && let Ok(active) = environments::resolve(&current)
                    {
                        resolved.push(active);
                    }
                    let envs: Vec<_> = resolved
                        .into_iter()
                        .map(|e| {
                            json!({
                                "name": e.name,
                                "active": e.name == current,
                                "apiUrl": e.api_url,
                            })
                        })
                        .collect();
                    Ok(CommandResult::new(json!(envs)))
                },
            ))
            .with_command(RuntimeCommandSpec::new(
                CommandSpec::new("get", "Get the active environment")
                    .with_system("env")
                    .with_tier(Tier::Read)
                    .no_auth(true),
                |_cred, _args| async move {
                    let env = active_env();
                    let resolved = environments::resolve(&env).map_err(map_err)?;
                    Ok(CommandResult::new(json!({
                        "env": resolved.name,
                        "apiUrl": resolved.api_url,
                    })))
                },
            ))
            .with_command(RuntimeCommandSpec::new_with_context(
                CommandSpec::new("set", "Set the active environment")
                    .with_system("env")
                    .with_tier(Tier::Mutate)
                    .no_auth(true)
                    .with_arg(
                        // Distinct id from the global `--env` flag (also id "env");
                        // a shared id makes the positional collide with the flag's
                        // default, so `env set <X>` would ignore <X>.
                        clap::Arg::new("environment")
                            .value_name("ENV")
                            .required(true)
                            .help("Environment to activate (ote|prod)"),
                    ),
                |ctx| async move {
                    let env = ctx
                        .args
                        .get("environment")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_owned();
                    // Resolve up front: validates the env exists (built-in, env
                    // var, or local config) and yields its API URL for the reply.
                    let resolved = environments::resolve(&env).map_err(map_err)?;
                    set_env(&env).map_err(|e| {
                        cli_engine::CliCoreError::message(format!(
                            "failed to write .gdenv state file: {e}"
                        ))
                    })?;
                    Ok(CommandResult::new(json!({
                        "env": resolved.name,
                        "apiUrl": resolved.api_url,
                    })))
                },
            ))
            .with_command(RuntimeCommandSpec::new(
                CommandSpec::new("info", "Show details for the active environment")
                    .with_system("env")
                    .with_tier(Tier::Read)
                    .no_auth(true),
                |_cred, _args| async move {
                    let env = active_env();
                    let resolved = environments::resolve(&env).map_err(map_err)?;
                    Ok(CommandResult::new(json!({
                        "env": resolved.name,
                        "apiUrl": resolved.api_url,
                        "graphqlUrl": format!("{}/v1/applications/graphql", resolved.api_url),
                    })))
                },
            ))
    })
}

#[cfg(test)]
mod tests {
    use cli_engine::{Cli, CliConfig};

    /// `env list` is a local-only command: it must run without any auth flow.
    ///
    /// The CLI is built with **no auth provider registered**. Because the engine
    /// authenticates fail-closed by default (`AuthRequirement::Required`), a
    /// command that forgot to opt out would fail here with "no provider
    /// registered". This guards that the `env` commands stay `no_auth(true)`.
    #[tokio::test]
    async fn env_list_runs_without_auth() {
        let cli = Cli::new(
            CliConfig::new("gddy", "GoDaddy developer CLI", "gddy").with_module(super::module()),
        );

        let output = cli.run(["gddy", "env", "list", "--output", "json"]).await;

        assert_eq!(output.exit_code, 0, "rendered output: {}", output.rendered);
        let json: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json output");
        let envs = json["data"].as_array().expect("data array");
        // Both built-ins are always listed, regardless of local config / env vars.
        for name in ["ote", "prod"] {
            let entry = envs
                .iter()
                .find(|e| e["name"] == name)
                .unwrap_or_else(|| unreachable!("missing env {name} in: {}", output.rendered));
            // Output-shape contract: each entry carries name/active/apiUrl.
            assert!(
                entry["active"].is_boolean(),
                "active should be bool: {entry}"
            );
            assert!(
                entry["apiUrl"].as_str().is_some_and(|u| u.contains("://")),
                "apiUrl should be a URL: {entry}"
            );
        }
    }
}
