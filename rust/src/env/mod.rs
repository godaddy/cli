use cli_engine::{
    CommandResult, CommandSpec, GroupSpec, Module, RuntimeCommandSpec, RuntimeGroupSpec, Tier,
};
use serde_json::json;

const OTE_API_URL: &str = "https://api.ote-godaddy.com";
const PROD_API_URL: &str = "https://api.godaddy.com";
const KNOWN_ENVS: &[&str] = &["ote", "prod"];
pub const DEFAULT_ENV: &str = "prod";

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
        // unknown environment that helpers would silently treat as prod.
        .filter(|s| KNOWN_ENVS.contains(&s.as_str()))
}

pub fn set_env(env: &str) -> std::io::Result<()> {
    let path = gdenv_path()?;
    std::fs::write(&path, env).map_err(|e| {
        // Include the resolved (platform-dependent) path so callers can surface
        // an actionable message — the raw write error omits it.
        std::io::Error::new(e.kind(), format!("{}: {e}", path.display()))
    })
}

fn api_url_for(env: &str) -> &'static str {
    match env {
        "ote" => OTE_API_URL,
        _ => PROD_API_URL,
    }
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
                    let current = get_env().unwrap_or_else(|| DEFAULT_ENV.to_owned());
                    let envs: Vec<_> = KNOWN_ENVS
                        .iter()
                        .map(|&e| {
                            json!({
                                "name": e,
                                "active": e == current,
                                "apiUrl": api_url_for(e),
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
                    let env = get_env().unwrap_or_else(|| DEFAULT_ENV.to_owned());
                    Ok(CommandResult::new(json!({
                        "env": env,
                        "apiUrl": api_url_for(&env),
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
                    if !KNOWN_ENVS.contains(&env.as_str()) {
                        return Err(cli_engine::CliCoreError::message(format!(
                            "unknown environment {env:?}; expected one of: {}",
                            KNOWN_ENVS.join(", ")
                        )));
                    }
                    set_env(&env).map_err(|e| {
                        cli_engine::CliCoreError::message(format!(
                            "failed to write .gdenv state file: {e}"
                        ))
                    })?;
                    Ok(CommandResult::new(json!({
                        "env": env,
                        "apiUrl": api_url_for(&env),
                    })))
                },
            ))
            .with_command(RuntimeCommandSpec::new(
                CommandSpec::new("info", "Show details for the active environment")
                    .with_system("env")
                    .with_tier(Tier::Read)
                    .no_auth(true),
                |_cred, _args| async move {
                    let env = get_env().unwrap_or_else(|| DEFAULT_ENV.to_owned());
                    Ok(CommandResult::new(json!({
                        "env": env,
                        "apiUrl": api_url_for(&env),
                        "graphqlUrl": format!("{}/v1/applications/graphql", api_url_for(&env)),
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
        assert!(
            envs.iter()
                .any(|e| e["name"] == "ote" || e["name"] == "prod"),
            "expected known environments in output, got: {}",
            output.rendered
        );
    }
}
