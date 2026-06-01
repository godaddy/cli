use cli_engine::{CommandResult, CommandSpec, GroupSpec, Module, RuntimeCommandSpec, RuntimeGroupSpec, Tier};
use serde_json::json;

const OTE_API_URL: &str = "https://api.ote-godaddy.com";
const PROD_API_URL: &str = "https://api.godaddy.com";
const KNOWN_ENVS: &[&str] = &["ote", "prod"];

fn gdenv_path() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    std::path::PathBuf::from(home).join(".gdenv")
}

pub fn get_env() -> Option<String> {
    std::fs::read_to_string(gdenv_path())
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
}

pub fn set_env(env: &str) -> std::io::Result<()> {
    std::fs::write(gdenv_path(), env)
}

fn api_url_for(env: &str) -> &'static str {
    match env {
        "prod" => PROD_API_URL,
        _ => OTE_API_URL,
    }
}

pub fn module() -> Module {
    Module::new("Admin", |_ctx| {
        RuntimeGroupSpec::new(GroupSpec::new("env", "Manage active GoDaddy environment"))
            .with_command(RuntimeCommandSpec::new(
                CommandSpec::new("list", "List available environments")
                    .with_system("env")
                    .with_tier(Tier::Read),
                |_cred, _args| async move {
                    let current = get_env().unwrap_or_else(|| "ote".to_owned());
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
                    .with_tier(Tier::Read),
                |_cred, _args| async move {
                    let env = get_env().unwrap_or_else(|| "ote".to_owned());
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
                    .with_arg(
                        clap::Arg::new("env")
                            .value_name("ENV")
                            .required(true)
                            .help("Environment to activate (ote|prod)"),
                    ),
                |ctx| async move {
                    let env = ctx
                        .args
                        .get("env")
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
                        cli_engine::CliCoreError::message(format!("failed to write ~/.gdenv: {e}"))
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
                    .with_tier(Tier::Read),
                |_cred, _args| async move {
                    let env = get_env().unwrap_or_else(|| "ote".to_owned());
                    Ok(CommandResult::new(json!({
                        "env": env,
                        "apiUrl": api_url_for(&env),
                        "graphqlUrl": format!("{}/v1/applications/graphql", api_url_for(&env)),
                    })))
                },
            ))
    })
}
