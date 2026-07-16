//! `gddy auth scopes` — list every OAuth scope the CLI can request, and which
//! commands need it.
//!
//! Lets an agent driving a multi-step workflow discover every scope it will
//! need *before* starting, so it can do a single eager `auth login --scope
//! ...` up front instead of hitting cli-engine's interactive step-up prompt
//! mid-workflow (a hard blocker for a non-interactive agent).

use std::collections::BTreeMap;

use cli_engine::{CliCoreError, CommandResult, CommandSpec, RuntimeCommandSpec, Tier};
use serde_json::json;

use crate::output_schema::output_schema;
use crate::scopes::{SCOPE_REGISTRY, command_scopes};

output_schema!(ScopeEntry {
    "scope": "string";
    "description": "string";
    "default": "bool";
    "commands": "[]string";
});

/// A [`SCOPE_REGISTRY`] row joined with the commands that declare it live via
/// `.with_scopes`, from [`command_scopes`].
struct ScopeEntryData {
    scope: &'static str,
    description: &'static str,
    default: bool,
    commands: Vec<String>,
}

impl ScopeEntryData {
    fn to_json(&self) -> serde_json::Value {
        json!({
            "scope": self.scope,
            "description": self.description,
            "default": self.default,
            "commands": self.commands,
        })
    }
}

/// Joins [`SCOPE_REGISTRY`] with the live scope→command correlation (from
/// [`command_scopes`], passed in by the caller so a single invocation walks
/// the command tree only once), sorted by [`SCOPE_REGISTRY`]'s declaration
/// order for stable output.
fn build_entries(command_scopes: &[(String, Vec<String>)]) -> Vec<ScopeEntryData> {
    let mut commands_by_scope: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (path, scopes) in command_scopes {
        for scope in scopes {
            commands_by_scope
                .entry(scope.clone())
                .or_default()
                .push(path.clone());
        }
    }
    SCOPE_REGISTRY
        .iter()
        .map(|info| ScopeEntryData {
            scope: info.scope,
            description: info.description,
            default: info.default,
            commands: commands_by_scope
                .get(info.scope)
                .cloned()
                .unwrap_or_default(),
        })
        .collect()
}

pub(crate) fn auth_scopes_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_with_context(
        CommandSpec::new("scopes", "List requestable OAuth scopes")
            .with_long(
                "List every OAuth scope the CLI can request, its description, whether \
                 it's requested at login by default, and which commands need it.\n\
                 \n\
                 Useful for planning an eager login before a multi-step workflow: run \
                 `gddy auth scopes --non-default` to see which scopes require an \
                 explicit `--scope`, then pass each one to `gddy auth login --scope <s>` \
                 up front to avoid an interactive step-up prompt mid-workflow.",
            )
            .with_system("auth")
            .with_tier(Tier::Read)
            .with_output_schema::<ScopeEntry>()
            .no_auth(true)
            .with_arg(
                clap::Arg::new("command")
                    .long("command")
                    .value_name("PATH")
                    .conflicts_with_all(["defaults-only", "non-default"])
                    .help("Only show scopes required by this command (e.g. \"domain purchase\")"),
            )
            .with_arg(
                clap::Arg::new("defaults-only")
                    .long("defaults-only")
                    .action(clap::ArgAction::SetTrue)
                    .conflicts_with("non-default")
                    .help("Only show scopes requested at login by default"),
            )
            .with_arg(
                clap::Arg::new("non-default")
                    .long("non-default")
                    .action(clap::ArgAction::SetTrue)
                    .help("Only show scopes that require an explicit --scope at login"),
            ),
        |ctx| async move {
            let scopes_by_command = command_scopes();
            let entries = build_entries(&scopes_by_command);
            let filtered = if let Some(path) = ctx
                .args
                .get("command")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
            {
                if !scopes_by_command.iter().any(|(p, _)| p == path) {
                    return Err(CliCoreError::message(format!(
                        "unknown command {path:?}; run `gddy tree` to see available commands"
                    )));
                }
                entries
                    .into_iter()
                    .filter(|e| e.commands.iter().any(|c| c == path))
                    .collect::<Vec<_>>()
            } else if ctx
                .args
                .get("defaults-only")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                entries.into_iter().filter(|e| e.default).collect()
            } else if ctx
                .args
                .get("non-default")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                entries.into_iter().filter(|e| !e.default).collect()
            } else {
                entries
            };
            let rendered: Vec<_> = filtered.iter().map(ScopeEntryData::to_json).collect();
            Ok(CommandResult::new(json!(rendered)))
        },
    )
}

#[cfg(test)]
mod tests {
    use cli_engine::{Cli, CliConfig};

    use super::auth_scopes_command;

    fn cli() -> Cli {
        Cli::new(
            CliConfig::new("gddy", "GoDaddy developer CLI", "gddy")
                // `auth` is only mounted when a default provider is configured
                // (cli-engine's `ensure_auth_command`); no provider needs to
                // actually be registered since `scopes` is `no_auth(true)`.
                .with_default_auth_provider("godaddy")
                .with_modules(crate::all_modules())
                .with_auth_extra_commands([auth_scopes_command()]),
        )
    }

    /// `auth scopes` is pure static/derived data: it must run with no auth
    /// provider registered at all, just like `auth status`.
    #[tokio::test]
    async fn auth_scopes_runs_without_auth() {
        let output = cli()
            .run(["gddy", "auth", "scopes", "--output", "json"])
            .await;
        assert_eq!(output.exit_code, 0, "rendered output: {}", output.rendered);
    }

    /// Every scope in the CLI's registry is listed, and `domain purchase`'s
    /// entry matches its two live `.with_scopes` declarations
    /// (`domains.domain:read`, `domains.domain:create`) — the exact example
    /// called out in the DEVEX-886/891 plan.
    #[tokio::test]
    async fn auth_scopes_lists_every_scope_with_live_commands() {
        let output = cli()
            .run(["gddy", "auth", "scopes", "--output", "json"])
            .await;
        assert_eq!(output.exit_code, 0, "rendered output: {}", output.rendered);
        let json: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json output");
        let entries = json["data"].as_array().expect("data array");
        assert_eq!(entries.len(), crate::scopes::SCOPE_REGISTRY.len());

        let create = entries
            .iter()
            .find(|e| e["scope"] == "domains.domain:create")
            .expect("domains.domain:create entry present");
        let commands: Vec<&str> = create["commands"]
            .as_array()
            .expect("commands array")
            .iter()
            .map(|c| c.as_str().expect("command is a string"))
            .collect();
        assert!(commands.contains(&"domain purchase"));
    }

    /// `--command` is a reverse lookup: given a command path, return exactly
    /// the scopes it declares.
    #[tokio::test]
    async fn auth_scopes_command_filter_is_a_reverse_lookup() {
        let output = cli()
            .run([
                "gddy",
                "auth",
                "scopes",
                "--command",
                "domain purchase",
                "--output",
                "json",
            ])
            .await;
        assert_eq!(output.exit_code, 0, "rendered output: {}", output.rendered);
        let json: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json output");
        let scopes: std::collections::BTreeSet<&str> = json["data"]
            .as_array()
            .expect("data array")
            .iter()
            .map(|e| e["scope"].as_str().expect("scope is a string"))
            .collect();
        assert_eq!(
            scopes,
            std::collections::BTreeSet::from(["domains.domain:read", "domains.domain:create"])
        );
    }

    /// An unknown `--command` path is a user error, not a silently empty list.
    #[tokio::test]
    async fn auth_scopes_command_filter_rejects_unknown_path() {
        let output = cli()
            .run([
                "gddy",
                "auth",
                "scopes",
                "--command",
                "does not exist",
                "--output",
                "json",
            ])
            .await;
        assert_ne!(output.exit_code, 0, "rendered output: {}", output.rendered);
    }

    /// `--defaults-only` and `--non-default` partition the full set.
    #[tokio::test]
    async fn auth_scopes_defaults_only_and_non_default_partition_the_registry() {
        let defaults = cli()
            .run([
                "gddy",
                "auth",
                "scopes",
                "--defaults-only",
                "--output",
                "json",
            ])
            .await;
        let non_default = cli()
            .run([
                "gddy",
                "auth",
                "scopes",
                "--non-default",
                "--output",
                "json",
            ])
            .await;
        assert_eq!(defaults.exit_code, 0, "{}", defaults.rendered);
        assert_eq!(non_default.exit_code, 0, "{}", non_default.rendered);

        let defaults_json: serde_json::Value =
            serde_json::from_str(&defaults.rendered).expect("valid json");
        let non_default_json: serde_json::Value =
            serde_json::from_str(&non_default.rendered).expect("valid json");
        let defaults_count = defaults_json["data"].as_array().expect("array").len();
        let non_default_count = non_default_json["data"].as_array().expect("array").len();
        assert_eq!(
            defaults_count + non_default_count,
            crate::scopes::SCOPE_REGISTRY.len()
        );
        assert!(
            defaults_json["data"]
                .as_array()
                .expect("array")
                .iter()
                .all(|e| e["default"] == true),
        );
        assert!(
            non_default_json["data"]
                .as_array()
                .expect("array")
                .iter()
                .all(|e| e["default"] == false),
        );
    }
}
