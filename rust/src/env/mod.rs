use cli_engine::{
    CommandResult, CommandSpec, GroupSpec, Module, RuntimeCommandSpec, RuntimeGroupSpec, Tier,
};
use serde_json::json;

use crate::environments::{self, EnvError};
use crate::output_schema::output_schema;

output_schema!(EnvSummary {
    "name": "string";
    "active": "bool";
    "apiUrl": "string";
});

output_schema!(EnvActive {
    "env": "string";
    "apiUrl": "string";
});

// `env set` always returns an "action" ("activated" for real, "would
// activate" under --dry-run) — a dedicated schema, rather than adding
// `action` to `EnvActive`, since `EnvActive` is shared with `env get`, which
// never has an action concept.
output_schema!(EnvSetResult {
    "env": "string";
    "apiUrl": "string";
    "action": "string";
});

output_schema!(EnvInfo {
    "env": "string";
    "apiUrl": "string";
    "graphqlUrl": "string";
});

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
        // survives — except a config-only env is dropped if the config file
        // can't be read, since `is_known` then falls back to built-ins +
        // `<ENV>_API_URL` only.
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
        RuntimeGroupSpec::new(
            GroupSpec::new("env", "Manage active GoDaddy environment").with_long(
                "Control which GoDaddy environment (e.g. prod, ote) all commands talk to.\n\
                     The active environment determines the API endpoints used for \
                     every request.\n\
                     Use `gddy env set` to switch environments; the selection is \
                     persisted to ~/.gdenv and survives new shell sessions.",
            ),
        )
        .with_command(RuntimeCommandSpec::new(
            CommandSpec::new("list", "List available environments")
                .with_long(
                    "Lists the environments the CLI can target (prod, ote). \
                         The currently active environment is marked `active: true`.",
                )
                .with_system("env")
                .with_tier(Tier::Read)
                .with_output_schema::<EnvSummary>()
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
                .with_long(
                    "Prints the name and API base URL of the currently active \
                         environment.\n\
                         If no environment has been set with `gddy env set`, \
                         the default environment is used.",
                )
                .with_system("env")
                .with_tier(Tier::Read)
                .with_output_schema::<EnvActive>()
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
                .with_long(
                    "Switches the active environment and persists the choice to \
                         ~/.gdenv so all subsequent commands use the new environment \
                         without needing `--env`.\n\
                         The value must be a known environment name (prod or ote).",
                )
                .with_system("env")
                .with_tier(Tier::Mutate)
                .handles_dry_run(true)
                .with_output_schema::<EnvSetResult>()
                .no_auth(true)
                .with_arg(
                    // Distinct id from the global `--env` flag (also id "env");
                    // a shared id makes the positional collide with the flag's
                    // default, so `env set <X>` would ignore <X>.
                    clap::Arg::new("environment")
                        .value_name("ENV")
                        .required(true)
                        .help("Environment name to activate (prod or ote)"),
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
                // Runs unconditionally, including under `--dry-run`.
                let resolved = environments::resolve(&env).map_err(map_err)?;
                if ctx.dry_run() {
                    return Ok(CommandResult::new(json!({
                        "env": resolved.name,
                        "apiUrl": resolved.api_url,
                        "action": "would activate",
                    }))
                    .with_dry_run());
                }
                set_env(&env).map_err(|e| {
                    cli_engine::CliCoreError::message(format!(
                        "failed to write .gdenv state file: {e}"
                    ))
                })?;
                Ok(CommandResult::new(json!({
                    "env": resolved.name,
                    "apiUrl": resolved.api_url,
                    "action": "activated",
                })))
            },
        ))
        .with_command(RuntimeCommandSpec::new(
            CommandSpec::new("info", "Show details for the active environment")
                .with_long(
                    "Prints extended details for the active environment, \
                         including both the REST API base URL and the GraphQL \
                         endpoint used by application commands.\n\
                         Use `gddy env get` for a shorter summary.",
                )
                .with_system("env")
                .with_tier(Tier::Read)
                .with_output_schema::<EnvInfo>()
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

    /// `env set --dry-run` never calls `set_env`, so this is safe to run
    /// without touching the real `~/.gdenv` state file.
    #[tokio::test]
    async fn env_set_dry_run_previews_a_valid_environment_without_persisting() {
        let cli = Cli::new(
            CliConfig::new("gddy", "GoDaddy developer CLI", "gddy").with_module(super::module()),
        );

        let output = cli
            .run(["gddy", "env", "set", "ote", "--dry-run", "--output", "json"])
            .await;

        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let json: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json output");
        assert_eq!(json["data"]["env"], "ote");
        assert_eq!(json["data"]["action"], "would activate");
    }

    #[tokio::test]
    async fn env_set_dry_run_still_rejects_an_unknown_environment() {
        let cli = Cli::new(
            CliConfig::new("gddy", "GoDaddy developer CLI", "gddy").with_module(super::module()),
        );

        let output = cli
            .run([
                "gddy",
                "env",
                "set",
                "not-a-real-env",
                "--dry-run",
                "--output",
                "json",
            ])
            .await;

        assert_ne!(output.exit_code, 0, "{}", output.rendered);
    }
}
