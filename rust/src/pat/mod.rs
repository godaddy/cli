//! `gddy pat` — manage Personal Access Tokens (PATs) for non-interactive auth.
//!
//! PATs complement the interactive OAuth PKCE flow managed by `gddy auth`. They
//! are long-lived OAuth refresh tokens created in the GoDaddy Developer Portal
//! and used as `Authorization: Bearer gd_pat_…`. The gateway exchanges the PAT
//! for a short-lived access token, so the CLI only needs to store and send the
//! PAT itself.
//!
//! PATs are persisted in the `gddy` configuration directory alongside other CLI
//! configuration files (e.g. `~/.config/gddy/pat.toml` on Linux). They are written
//! with owner-only file permissions where the platform supports it. They can also
//! be supplied at runtime via the `GDDY_PAT` or `GDDY_PAT_<ENV>` environment
//! variables, which take precedence over the registry file.

use std::collections::BTreeMap;

use cli_engine::{
    CliCoreError, CommandResult, CommandSpec, GroupSpec, Module, RuntimeCommandSpec,
    RuntimeGroupSpec, Tier,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::environments;
use crate::next_action::next_action;
use crate::output_schema::output_schema;

output_schema!(PatListItem {
    "env": "string";
    "name": "string";
    "lastFour": "string";
});

output_schema!(PatAddResult {
    "env": "string";
    "name": "string";
    "lastFour": "string";
    "path": "string";
    "action": "string";
});

output_schema!(PatRemoveResult {
    "env": "string";
    "status": "string";
});

/// PAT prefix advertised by GoDaddy. A valid PAT starts with this string.
const PAT_PREFIX: &str = "gd_pat_";

/// Default env-var PAT applied to any environment.
pub const PAT_ENV_VAR: &str = "GDDY_PAT";

/// Provider name reported in `Credential.provider` when a PAT is used.
pub const PROVIDER: &str = "pat";

const PAT_FILE_NAME: &str = "pat.toml";

/// One stored PAT.
#[derive(Clone, Serialize, Deserialize)]
pub struct PatEntry {
    /// Plaintext PAT. Never displayed in full after initial entry.
    pub token: String,
    /// Human-readable label chosen when the PAT was added.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
}

impl std::fmt::Debug for PatEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PatEntry")
            .field("token", &redact(&self.token))
            .field("name", &self.name)
            .finish()
    }
}

/// Registry of PATs keyed by environment name.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PatRegistry {
    #[serde(default)]
    tokens: BTreeMap<String, PatEntry>,
}

impl PatRegistry {
    /// Load the PAT registry from disk, or an empty registry if the file is missing.
    /// A malformed file is treated as an error so a user notices and can fix it.
    fn load(path: &std::path::Path) -> Result<Self, CliCoreError> {
        match std::fs::read_to_string(path) {
            Ok(contents) => toml::from_str(&contents)
                .map_err(|e| CliCoreError::message(format!("{}: {e}", path.display()))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(CliCoreError::message(format!("{}: {e}", path.display()))),
        }
    }

    /// Save the registry to disk atomically with owner-only permissions.
    fn save(&self, path: &std::path::Path) -> Result<(), CliCoreError> {
        let contents = toml::to_string_pretty(self)
            .map_err(|e| CliCoreError::message(format!("failed to serialize PAT registry: {e}")))?;
        cli_engine::fs::write_string_atomic(path, &contents)
    }
}

/// Path to the local PAT registry file, if a config directory can be resolved.
/// Mirrors the other `gddy/` config paths in this crate.
pub fn registry_path() -> Option<std::path::PathBuf> {
    dirs::config_dir().map(|d| d.join("gddy").join(PAT_FILE_NAME))
}

fn env_var_for(env: &str) -> String {
    format!("{}_{}", PAT_ENV_VAR, environments::env_prefix(env))
}

fn load_registry(path: &std::path::Path) -> Result<PatRegistry, CliCoreError> {
    PatRegistry::load(path)
}

fn save_registry(path: &std::path::Path, registry: &PatRegistry) -> Result<(), CliCoreError> {
    registry.save(path)
}

/// Validate that `token` looks like a GoDaddy PAT. This only checks for the
/// `gd_pat_` prefix plus at least one character of content after it with no
/// embedded whitespace, so that we are not overly tied to the implementation
/// details of the token format while still rejecting untrimmed values (e.g.
/// `gd_pat_abc\n` from an env var or file with a trailing newline) that would
/// otherwise be sent as a garbage Bearer token.
#[must_use]
pub fn is_valid_pat(token: &str) -> bool {
    token
        .strip_prefix(PAT_PREFIX)
        .is_some_and(|rest| !rest.is_empty() && !rest.chars().any(char::is_whitespace))
}

/// Returns the last four characters of a PAT. If the token is four characters or
/// shorter, returns the whole token unchanged.
fn last_four(token: &str) -> String {
    token
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}

fn redact(token: &str) -> String {
    let len = token.chars().count();
    if len <= 8 {
        return "*".repeat(len);
    }
    let prefix: String = token.chars().take(4).collect();
    let suffix: String = token
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("{prefix}****{suffix}")
}

/// Find a PAT for `env`, preferring per-env env vars, then the default env var,
/// then the registry file. Returns `None` when no valid PAT is configured.
pub async fn resolve_pat(env: &str) -> Option<PatEntry> {
    // Check env vars before touching the registry. This keeps the registry-load
    // warning honest (env vars have already been ruled out) and avoids I/O when
    // a PAT is supplied via the environment.
    if let Some(entry) = resolve_pat_with(env, |var| std::env::var(var).ok(), None) {
        return Some(entry);
    }
    let path = registry_path()?;
    let registry = match load_registry(&path) {
        Ok(r) => r,
        Err(err) => {
            tracing::warn!(
                env,
                error = %err,
                "failed to load PAT registry; falling back to OAuth"
            );
            return None;
        }
    };
    resolve_pat_with(env, |_| None, Some(&registry))
}

/// Internal, testable version of [`resolve_pat`]. `get_env` supplies env-var values;
/// `registry` is the pre-loaded PAT registry (if any). Every token is validated
/// before it is returned.
fn resolve_pat_with<F>(
    env: &str,
    mut get_env: F,
    registry: Option<&PatRegistry>,
) -> Option<PatEntry>
where
    F: FnMut(&str) -> Option<String>,
{
    if let Some(token) = get_env(&env_var_for(env)) {
        if is_valid_pat(&token) {
            return Some(PatEntry {
                token,
                name: "env".to_owned(),
            });
        }
        tracing::warn!(
            env,
            var = env_var_for(env),
            "PAT env var is malformed; ignoring"
        );
    }
    if let Some(token) = get_env(PAT_ENV_VAR) {
        if is_valid_pat(&token) {
            return Some(PatEntry {
                token,
                name: "env".to_owned(),
            });
        }
        tracing::warn!(var = PAT_ENV_VAR, "PAT env var is malformed; ignoring");
    }
    let entry = registry?.tokens.get(env).cloned()?;
    if !is_valid_pat(&entry.token) {
        tracing::warn!(env, "stored PAT entry is malformed; ignoring");
        return None;
    }
    Some(entry)
}

/// Persist a PAT for an environment, replacing any existing entry.
pub async fn save_pat(env: &str, entry: PatEntry) -> Result<(), CliCoreError> {
    let path = registry_path().ok_or_else(|| {
        CliCoreError::message("could not determine a config directory for the PAT registry")
    })?;
    let mut registry = load_registry(&path)?;
    registry.tokens.insert(env.to_owned(), entry);
    save_registry(&path, &registry)
}

/// Remove a stored PAT for an environment. Returns `true` if one existed.
pub async fn delete_pat(env: &str) -> Result<bool, CliCoreError> {
    let Some(path) = registry_path() else {
        return Ok(false);
    };
    let mut registry = load_registry(&path)?;
    let existed = registry.tokens.remove(env).is_some();
    if existed {
        save_registry(&path, &registry)?;
    }
    Ok(existed)
}

/// Returns whether a PAT is currently stored for `env`, without removing it.
/// Used to preview `pat remove --dry-run` without mutating the registry.
pub fn has_pat(env: &str) -> Result<bool, CliCoreError> {
    let Some(path) = registry_path() else {
        return Ok(false);
    };
    Ok(load_registry(&path)?.tokens.contains_key(env))
}

/// List environments that have a PAT in the registry.
pub async fn list_pats() -> Result<Vec<(String, PatEntry)>, CliCoreError> {
    let Some(path) = registry_path() else {
        return Ok(Vec::new());
    };
    let registry = load_registry(&path)?;
    Ok(registry.tokens.into_iter().collect())
}

/// Return the environment names that have a stored PAT in the registry.
pub async fn registry_envs() -> Result<Vec<String>, CliCoreError> {
    let Some(path) = registry_path() else {
        return Ok(Vec::new());
    };
    let registry = load_registry(&path)?;
    Ok(registry.tokens.into_keys().collect())
}

fn registry_path_err() -> Result<std::path::PathBuf, CliCoreError> {
    registry_path().ok_or_else(|| {
        CliCoreError::message("could not determine a config directory for the PAT registry")
    })
}

pub fn module() -> Module {
    Module::new("Admin", |_ctx| {
        RuntimeGroupSpec::new(
            GroupSpec::new("pat", "Manage Personal Access Tokens (PATs)").with_long(
                "Store and manage Personal Access Tokens for non-interactive GoDaddy \
                         authentication. PATs are created in the Developer Portal and are useful \
                         for CI/CD pipelines and scripts where browser-based OAuth is not \
                         possible.\n\
                         \n\
                         • add     — store a PAT for an environment (reads from stdin)\n\
                         • list    — show stored PATs (last-four only)\n\
                         • remove  — delete the PAT for an environment\n\
                         \n\
                         PATs can also be supplied with the GDDY_PAT or GDDY_PAT_<ENV> \
                         environment variables. See `gddy guide auth`.",
            ),
        )
        .with_command(add_command())
        .with_command(list_command())
        .with_command(remove_command())
    })
    .with_guides_from_markdown([("auth.md", include_bytes!("guides/auth.md").as_slice())])
}

fn add_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_with_context(
        CommandSpec::new("add", "Store a PAT for an environment")
            .with_long(
                "Stores a Personal Access Token for the target environment.\n\
                 The token is read from stdin by default so it does not appear in shell \
                 history. Alternatively, pass --token.\n\
                 \n\
                 Example:\n  \
                   echo 'gd_pat_...' | gddy pat add --env prod \"CI token\"",
            )
            .with_system("pat")
            .with_tier(Tier::Mutate)
            .mutates(true)
            .handles_dry_run(true)
            .no_auth(true)
            .with_output_schema::<PatAddResult>()
            .with_default_fields("env,name,lastFour,path,action")
            .with_arg(
                clap::Arg::new("env")
                    .long("env")
                    .value_name("ENV")
                    .required(true)
                    .help("Environment the PAT is for (e.g. prod or ote)"),
            )
            .with_arg(
                clap::Arg::new("token")
                    .long("token")
                    .value_name("TOKEN")
                    .help("PAT value (if omitted, read from stdin)"),
            )
            .with_arg(
                clap::Arg::new("name")
                    .value_name("NAME")
                    .required(true)
                    .help("Human-readable label for this PAT"),
            ),
        |ctx| async move {
            let env = string_arg(&ctx.args, "env");
            // Validate the environment up front so a typo does not stash a PAT
            // for a non-existent env. Runs unconditionally, including under
            // `--dry-run`, so `--dry-run` can be used to pre-validate a PAT.
            environments::resolve(&env).map_err(|e| CliCoreError::message(e.to_string()))?;

            let name = string_arg(&ctx.args, "name");
            let token = resolve_token_arg(&ctx.args).await?;
            if !is_valid_pat(&token) {
                return Err(CliCoreError::message(
                    "token doesn't look like a GoDaddy PAT; refusing to store it. Run `gddy guide auth` for details on creating PATs.",
                ));
            }

            if ctx.dry_run() {
                // Resolve and load the registry too, so a dry-run can't
                // report success in a state (no config dir, or an existing
                // pat.toml that fails to parse) where a real run — which
                // calls save_pat, which loads the registry before writing —
                // would immediately fail before ever getting this far.
                let path = registry_path_err()?;
                load_registry(&path)?;
                return Ok(CommandResult::new(json!({
                    "env": env,
                    "name": name,
                    "lastFour": last_four(&token),
                    "path": path.display().to_string(),
                    "action": "would store",
                }))
                .with_dry_run());
            }

            let entry = PatEntry { token, name };
            save_pat(&env, entry.clone()).await?;

            let path = registry_path_err()?;
            Ok(CommandResult::new(json!({
                "env": env,
                "name": entry.name,
                "lastFour": last_four(&entry.token),
                "path": path.display().to_string(),
                "action": "stored",
            }))
            .with_next_actions(vec![next_action("guide auth", "Learn about PAT auth")]))
        },
    )
}

fn list_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new(
        CommandSpec::new("list", "List stored PATs")
            .with_long(
                "Lists every PAT stored in the registry, showing only the last four \
                 characters of each token. Use `GDDY_PAT` or `GDDY_PAT_<ENV>` env vars to \
                 provide a PAT without storing it in the registry.",
            )
            .with_system("pat")
            .with_tier(Tier::Read)
            .no_auth(true)
            .with_output_schema::<PatListItem>()
            .with_default_fields("env,name,lastFour"),
        |_cred, _args| async move {
            let mut entries: Vec<_> = list_pats()
                .await?
                .into_iter()
                .map(|(env, entry)| {
                    json!({
                        "env": env,
                        "name": entry.name,
                        "lastFour": last_four(&entry.token),
                    })
                })
                .collect();
            entries.sort_by(|a, b| a["env"].as_str().cmp(&b["env"].as_str()));
            Ok(CommandResult::new(json!(entries)))
        },
    )
}

fn remove_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_with_context(
        CommandSpec::new("remove", "Remove the PAT for an environment")
            .with_long(
                "Deletes the stored PAT for the given environment. This does not revoke \
                 the PAT in the Developer Portal; it only removes the local copy.",
            )
            .with_system("pat")
            .with_tier(Tier::Mutate)
            .mutates(true)
            .handles_dry_run(true)
            .no_auth(true)
            .with_output_schema::<PatRemoveResult>()
            .with_default_fields("env,status")
            .with_arg(
                clap::Arg::new("env")
                    .long("env")
                    .value_name("ENV")
                    .required(true)
                    .help("Environment whose PAT should be removed"),
            ),
        |ctx| async move {
            let env = string_arg(&ctx.args, "env");
            // Validate the environment up front so a typo produces a clear error
            // instead of silently reporting "not found".
            environments::resolve(&env).map_err(|e| CliCoreError::message(e.to_string()))?;

            if ctx.dry_run() {
                let status = if has_pat(&env)? {
                    "would remove"
                } else {
                    "not found"
                };
                return Ok(
                    CommandResult::new(json!({ "env": env, "status": status })).with_dry_run()
                );
            }

            let existed = delete_pat(&env).await?;
            Ok(CommandResult::new(json!({
                "env": env,
                "status": if existed { "removed" } else { "not found" },
            })))
        },
    )
}

fn string_arg(args: &serde_json::Map<String, serde_json::Value>, name: &str) -> String {
    args.get(name)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_owned()
}

async fn read_stdin_token() -> Result<String, CliCoreError> {
    use tokio::io::AsyncBufReadExt as _;
    let stdin = tokio::io::stdin();
    let mut reader = tokio::io::BufReader::new(stdin);
    let mut line = String::new();
    let bytes_read = reader
        .read_line(&mut line)
        .await
        .map_err(|e| CliCoreError::message(format!("failed to read PAT from stdin: {e}")))?;
    parse_stdin_token(&line, bytes_read)
}

/// Validate that a line read from stdin actually contains a token.
/// Returns an actionable error on EOF or whitespace-only input.
fn parse_stdin_token(line: &str, bytes_read: usize) -> Result<String, CliCoreError> {
    if bytes_read == 0 || line.trim().is_empty() {
        return Err(CliCoreError::message(
            "no PAT provided on stdin; pass --token or pipe the PAT",
        ));
    }
    Ok(line.trim().to_owned())
}

/// Return the PAT supplied on the command line, trimmed of surrounding whitespace,
/// or read it from stdin when `--token` is absent or whitespace-only.
async fn resolve_token_arg(
    args: &serde_json::Map<String, serde_json::Value>,
) -> Result<String, CliCoreError> {
    if let Some(t) = args.get("token").and_then(|v| v.as_str()) {
        let trimmed = t.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_owned());
        }
    }
    read_stdin_token().await
}

#[cfg(test)]
mod tests {
    use cli_engine::{Cli, CliConfig};

    use super::*;

    #[test]
    fn rejects_tokens_without_the_pat_prefix() {
        assert!(!is_valid_pat(""));
        assert!(!is_valid_pat("notapat"));
        assert!(!is_valid_pat("gd_pat")); // missing trailing underscore
        assert!(!is_valid_pat("gd_pat_")); // nothing after the prefix
    }

    #[test]
    fn rejects_tokens_containing_whitespace_after_the_prefix() {
        // An untrimmed env var or file (e.g. a trailing newline) should not be
        // treated as valid — it would otherwise be sent as a garbage Bearer
        // token. This covers both whitespace-only tails and whitespace
        // embedded alongside real-looking content.
        assert!(!is_valid_pat("gd_pat_ "));
        assert!(!is_valid_pat("gd_pat_\n"));
        assert!(!is_valid_pat("gd_pat_\t\t"));
        assert!(!is_valid_pat("gd_pat_abc123\n"));
        assert!(!is_valid_pat("gd_pat_ abc123"));
        assert!(!is_valid_pat("gd_pat_abc 123"));
    }

    #[test]
    fn accepts_any_non_empty_value_after_the_prefix() {
        // The CLI does not assume a stable shape beyond the prefix, so these
        // (including the exact strings from DEVEX-889's bug report) all pass.
        assert!(is_valid_pat("gd_pat_abc123_1234abcd"));
        assert!(is_valid_pat("gd_pat_aA0bB1cC2_abcdef12"));
        assert!(is_valid_pat("gd_pat_1234567890abcdef1234567890abcdef"));
        assert!(is_valid_pat("gd_pat_YWJjZGVmZ2hpamtsbW5vcHFyc3R1dnd4eXo="));
        assert!(is_valid_pat(
            "gd_pat_abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGH"
        ));
    }

    #[test]
    fn last_four_extracts_suffix() {
        assert_eq!(last_four("gd_pat_abc123_1234abcd"), "abcd");
        assert_eq!(last_four("short"), "hort");
        assert_eq!(last_four("ab"), "ab");
    }

    #[test]
    fn redact_masks_prefix_and_suffix_and_handles_unicode() {
        assert_eq!(redact("gd_pat_abc123_1234abcd"), "gd_p****abcd");
        assert_eq!(redact("short"), "*****");
        // Multi-byte characters must not cause panics at byte boundaries.
        let unicode = "éééé_éééé_éééé";
        assert!(!redact(unicode).is_empty());
    }

    #[tokio::test]
    async fn cli_token_is_trimmed_before_validation() {
        let mut args = serde_json::Map::new();
        args.insert(
            "token".to_owned(),
            serde_json::Value::String("  gd_pat_a_12345678  \n".to_owned()),
        );
        let token = resolve_token_arg(&args).await.expect("token arg resolves");
        assert_eq!(token, "gd_pat_a_12345678");
        assert!(is_valid_pat(&token));
    }

    #[test]
    fn empty_stdin_token_returns_actionable_error() {
        assert!(parse_stdin_token("", 0).is_err());
        assert!(parse_stdin_token("  \n", 3).is_err());
        assert_eq!(
            parse_stdin_token("  gd_pat_a_12345678  \n", 23).expect("token parses"),
            "gd_pat_a_12345678"
        );
    }

    #[test]
    fn registry_round_trip() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("pat.toml");

        let mut registry = PatRegistry::default();
        registry.tokens.insert(
            "prod".to_owned(),
            PatEntry {
                token: "gd_pat_abc123_1234abcd".to_owned(),
                name: "CI".to_owned(),
            },
        );
        registry.save(&path).expect("save");

        let loaded = PatRegistry::load(&path).expect("load");
        assert_eq!(loaded.tokens.len(), 1);
        assert_eq!(loaded.tokens["prod"].name, "CI");
        assert_eq!(loaded.tokens["prod"].token, "gd_pat_abc123_1234abcd");
    }

    /// `pat add --dry-run` calls this same function so it can't report
    /// success in a state where a real run (which loads the registry via
    /// `save_pat`) would immediately fail parsing an existing malformed file.
    #[test]
    fn load_registry_rejects_malformed_toml() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("pat.toml");
        std::fs::write(&path, "this is not valid toml {{{").expect("write");
        let err = load_registry(&path).expect_err("malformed TOML should be rejected");
        assert!(err.to_string().contains("pat.toml"), "{err}");
    }

    fn test_registry_with(env: &str, token: &str) -> PatRegistry {
        let mut registry = PatRegistry::default();
        registry.tokens.insert(
            env.to_owned(),
            PatEntry {
                token: token.to_owned(),
                name: "stored".to_owned(),
            },
        );
        registry
    }

    #[test]
    fn per_env_var_wins_over_default_and_registry() {
        let registry = test_registry_with("prod", "gd_pat_registry_12345678");
        let env = |var: &str| match var {
            "GDDY_PAT_PROD" => Some("gd_pat_prod_12345678".to_owned()),
            "GDDY_PAT" => Some("gd_pat_default_12345678".to_owned()),
            _ => None,
        };
        let entry = resolve_pat_with("prod", env, Some(&registry)).expect("pat resolves");
        assert_eq!(entry.token, "gd_pat_prod_12345678");
        assert_eq!(entry.name, "env");
    }

    #[test]
    fn default_env_var_wins_over_registry() {
        let registry = test_registry_with("prod", "gd_pat_registry_12345678");
        let env = |var: &str| match var {
            "GDDY_PAT" => Some("gd_pat_default_12345678".to_owned()),
            _ => None,
        };
        let entry = resolve_pat_with("prod", env, Some(&registry)).expect("pat resolves");
        assert_eq!(entry.token, "gd_pat_default_12345678");
        assert_eq!(entry.name, "env");
    }

    #[test]
    fn malformed_env_var_is_ignored_and_fallback_works() {
        let registry = test_registry_with("prod", "gd_pat_registry_12345678");
        let env = |var: &str| match var {
            "GDDY_PAT_PROD" => Some("not-a-pat".to_owned()),
            "GDDY_PAT" => Some("gd_pat_default_12345678".to_owned()),
            _ => None,
        };
        let entry = resolve_pat_with("prod", env, Some(&registry)).expect("pat resolves");
        assert_eq!(entry.token, "gd_pat_default_12345678");
    }

    #[test]
    fn registry_entry_returned_when_no_env_vars() {
        let registry = test_registry_with("prod", "gd_pat_registry_12345678");
        let env = |_var: &str| None;
        let entry = resolve_pat_with("prod", env, Some(&registry)).expect("pat resolves");
        assert_eq!(entry.token, "gd_pat_registry_12345678");
        assert_eq!(entry.name, "stored");
    }

    #[test]
    fn stored_malformed_pat_is_ignored() {
        let registry = test_registry_with("prod", "not-a-pat");
        let env = |_var: &str| None;
        assert!(resolve_pat_with("prod", env, Some(&registry)).is_none());
    }

    #[test]
    fn redact_uses_char_boundaries_and_does_not_panic() {
        // Regression test: byte-index slicing would panic on this token.
        let token = "gd_pat_αβγδ_12345678";
        let redacted = redact(token);
        assert!(redacted.starts_with("gd_p"));
        assert!(redacted.ends_with("5678"));
        assert!(redacted.contains("****"));
    }

    #[test]
    fn debug_format_does_not_panic_on_multibyte_token() {
        let entry = PatEntry {
            token: "gd_pat_αβγδ_12345678".to_owned(),
            name: "test".to_owned(),
        };
        let _ = format!("{:?}", entry);
    }

    #[test]
    fn env_var_for_uses_uppercase_env_prefix() {
        assert_eq!(env_var_for("prod"), "GDDY_PAT_PROD");
        assert_eq!(env_var_for("ote"), "GDDY_PAT_OTE");
    }

    /// DEVEX-889: the message should point at `gddy guide auth` and not imply
    /// the user could type/guess a valid PAT by hand.
    #[tokio::test]
    async fn invalid_pat_message_points_to_guide_auth() {
        let cli =
            Cli::new(CliConfig::new("gddy", "GoDaddy developer CLI", "gddy").with_module(module()));
        let output = cli
            .run([
                "gddy", "pat", "add", "--env", "ote", "--token", "garbage", "test",
            ])
            .await;

        assert_ne!(output.exit_code, 0, "{}", output.rendered);
        assert!(
            output.rendered.contains("guide auth"),
            "{}",
            output.rendered
        );
        assert!(
            !output.rendered.contains("expected `gd_pat_...`"),
            "should not imply a literal typed format: {}",
            output.rendered
        );
    }

    /// GDDEVPLAT-81: `--dry-run` must still reject a malformed token instead of
    /// unconditionally reporting the generic "would execute".
    #[tokio::test]
    async fn add_dry_run_still_rejects_a_malformed_token() {
        let cli =
            Cli::new(CliConfig::new("gddy", "GoDaddy developer CLI", "gddy").with_module(module()));
        let output = cli
            .run([
                "gddy",
                "pat",
                "add",
                "--env",
                "ote",
                "--token",
                "garbage",
                "test",
                "--dry-run",
            ])
            .await;

        assert_ne!(output.exit_code, 0, "{}", output.rendered);
        assert!(
            !output.rendered.contains("would execute") && !output.rendered.contains("would store"),
            "a malformed token must be rejected, not previewed: {}",
            output.rendered
        );
    }

    /// GDDEVPLAT-81: a well-formed token previews without being stored.
    #[tokio::test]
    async fn add_dry_run_previews_a_valid_token_without_storing() {
        let cli =
            Cli::new(CliConfig::new("gddy", "GoDaddy developer CLI", "gddy").with_module(module()));
        let output = cli
            .run([
                "gddy",
                "pat",
                "add",
                "--env",
                "ote",
                "--token",
                "gd_pat_abc123_1234abcd",
                "test",
                "--dry-run",
                "--output",
                "json",
            ])
            .await;

        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let rendered: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json");
        assert_eq!(rendered["data"]["action"], "would store");
        assert_eq!(rendered["data"]["lastFour"], "abcd");
    }

    /// `pat remove --dry-run` previews existence without deleting.
    #[tokio::test]
    async fn remove_dry_run_reports_not_found_without_error() {
        let cli =
            Cli::new(CliConfig::new("gddy", "GoDaddy developer CLI", "gddy").with_module(module()));
        let output = cli
            .run([
                "gddy",
                "pat",
                "remove",
                "--env",
                "ote",
                "--dry-run",
                "--output",
                "json",
            ])
            .await;

        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let rendered: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json");
        let status = rendered["data"]["status"].as_str().unwrap_or_default();
        assert!(
            status == "would remove" || status == "not found",
            "unexpected status: {rendered}"
        );
    }
}
