//! Single source of truth for environment → endpoint resolution.
//!
//! Built-in, public-safe environments (`ote`, `prod`) are compiled in. Internal
//! DEV/TEST environments are supplied **at runtime** and never committed to this
//! (OSS) repo, via two override mechanisms:
//!
//! * **Per-env environment variable** — `<PREFIX>_API_URL` overrides (or defines)
//!   an environment's API base URL, where `<PREFIX>` is the env name uppercased
//!   with `-` replaced by `_` (e.g. `DEV_API_URL`, `OTE_API_URL`). This mirrors
//!   cli-engine's `<PREFIX>_OAUTH_CLIENT_ID` / `_AUTH_URL` / `_TOKEN_URL` naming,
//!   which `PkceAuthProvider` reads automatically when a provider is named after
//!   its environment.
//! * **Gitignored local config** — `~/.config/gddy/environments.toml` listing
//!   custom environments. The file lives in the user's home directory, never in
//!   the repo, so internal hostnames stay on the developer's machine.
//!
//! Resolution order (later layers win): built-in base → local config entry →
//! `<PREFIX>_API_URL` env var. Built-ins may be overridden by either layer.
//!
//! Security note: because a built-in's `api_url` is overridable (and `auth_url`
//! /`token_url` derive from it), overriding e.g. `prod` redirects that env's
//! OAuth and bearer traffic to the new host while still presenting the built-in
//! client id — i.e. a real prod token could be sent to the overriding host. The
//! override surface (a process env var or a file under the user's home dir)
//! already implies local trust, so this is an accepted trade-off; only override
//! a built-in on a machine you control.

use std::collections::BTreeMap;

use serde::Deserialize;

pub const DEFAULT_ENV: &str = "prod";
/// Scopes requested at login by default. The authorization server may grant a
/// subset; commands needing more declare them and the provider steps up.
pub const DEFAULT_OAUTH_SCOPES: &[&str] = &["apps.app-registry:read", "apps.app-registry:write"];
pub const REDIRECT_URI: &str = "http://localhost:7443/callback";
pub const APP_ID: &str = "gddy";

struct Builtin {
    name: &'static str,
    api_url: &'static str,
    client_id: &'static str,
}

/// Public-safe, compiled-in environments. `api.ote-godaddy.com` is public, and
/// these client IDs are public OAuth identifiers (not secrets).
const BUILTINS: &[Builtin] = &[
    Builtin {
        name: "ote",
        api_url: "https://api.ote-godaddy.com",
        client_id: "a502484b-d7b1-4509-aa88-08b391a54c28",
    },
    Builtin {
        name: "prod",
        api_url: "https://api.godaddy.com",
        client_id: "39489dee-4103-4284-9aab-9f2452142bce",
    },
];

/// A fully-resolved environment: everything needed to talk to it.
#[derive(Debug, Clone)]
pub struct ResolvedEnv {
    pub name: String,
    pub api_url: String,
    pub client_id: String,
    pub auth_url: String,
    pub token_url: String,
}

/// Schema of `~/.config/gddy/environments.toml`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct EnvironmentsFile {
    #[serde(default)]
    pub environments: BTreeMap<String, EnvEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EnvEntry {
    pub api_url: String,
    #[serde(default)]
    pub client_id: Option<String>,
    #[serde(default)]
    pub auth_url: Option<String>,
    #[serde(default)]
    pub token_url: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum EnvError {
    #[error("unknown environment {name:?}; known: {known}")]
    Unknown { name: String, known: String },
    #[error("failed to read {path}: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },
    #[error("failed to parse {path}: {source}")]
    Parse {
        path: String,
        source: toml::de::Error,
    },
}

/// Environment-variable prefix for an env name, matching cli-engine's
/// `PkceAuthProvider` derivation (uppercase, `-` → `_`).
fn env_prefix(name: &str) -> String {
    name.to_uppercase().replace('-', "_")
}

fn derive_auth_url(api_url: &str) -> String {
    format!("{}/v2/oauth2/authorize", api_url.trim_end_matches('/'))
}

fn derive_token_url(api_url: &str) -> String {
    format!("{}/v2/oauth2/token", api_url.trim_end_matches('/'))
}

/// Path to the local environments config file, if a config dir can be resolved.
///
/// Uses `dirs::config_dir()` which honors `XDG_CONFIG_HOME` (→ `~/.config`) on
/// Linux, matching cli-engine's own credential-store location logic.
pub fn environments_path() -> Option<std::path::PathBuf> {
    dirs::config_dir().map(|d| d.join("gddy").join("environments.toml"))
}

/// Load the local environments file. A missing file is not an error.
fn load_file() -> Result<EnvironmentsFile, EnvError> {
    let Some(path) = environments_path() else {
        return Ok(EnvironmentsFile::default());
    };
    match std::fs::read_to_string(&path) {
        Ok(contents) => toml::from_str(&contents).map_err(|source| EnvError::Parse {
            path: path.display().to_string(),
            source,
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(EnvironmentsFile::default()),
        Err(source) => Err(EnvError::Io {
            path: path.display().to_string(),
            source,
        }),
    }
}

fn builtin(name: &str) -> Option<&'static Builtin> {
    BUILTINS.iter().find(|b| b.name == name)
}

fn known_names(file: &EnvironmentsFile) -> String {
    let mut names: Vec<&str> = BUILTINS.iter().map(|b| b.name).collect();
    names.extend(file.environments.keys().map(String::as_str));
    names.sort_unstable();
    names.dedup();
    names.join(", ")
}

/// Resolve an environment from an explicit file + env-var getter. Pure: all
/// inputs are injected, so this is unit-testable without touching process state.
fn resolve_with(
    name: &str,
    file: &EnvironmentsFile,
    var: impl Fn(&str) -> Option<String>,
) -> Result<ResolvedEnv, EnvError> {
    // Layer 1: built-in base.
    let (mut api_url, mut client_id) = match builtin(name) {
        Some(b) => (Some(b.api_url.to_owned()), b.client_id.to_owned()),
        None => (None, String::new()),
    };
    let mut auth_url: Option<String> = None;
    let mut token_url: Option<String> = None;

    // Layer 2: local config entry (overrides/defines).
    if let Some(entry) = file.environments.get(name) {
        api_url = Some(entry.api_url.clone());
        if let Some(cid) = &entry.client_id {
            client_id = cid.clone();
        }
        auth_url = entry.auth_url.clone();
        token_url = entry.token_url.clone();
    }

    // Layer 3: per-env `<PREFIX>_API_URL` override (highest precedence).
    if let Some(url) = var(&format!("{}_API_URL", env_prefix(name))) {
        api_url = Some(url);
    }

    let api_url = api_url.ok_or_else(|| EnvError::Unknown {
        name: name.to_owned(),
        known: known_names(file),
    })?;
    // Normalize once so callers that concatenate (`{api_url}{endpoint}`,
    // `{api_url}/v1/...`) never produce a `//` path segment.
    let api_url = api_url.trim_end_matches('/').to_owned();

    let auth_url = auth_url.unwrap_or_else(|| derive_auth_url(&api_url));
    let token_url = token_url.unwrap_or_else(|| derive_token_url(&api_url));

    Ok(ResolvedEnv {
        name: name.to_owned(),
        api_url,
        client_id,
        auth_url,
        token_url,
    })
}

/// Names listed by `env list`: built-ins + locally-configured entries only
/// (env-var-only environments are intentionally excluded).
fn listable_with(
    file: &EnvironmentsFile,
    var: impl Fn(&str) -> Option<String> + Copy,
) -> Result<Vec<ResolvedEnv>, EnvError> {
    let mut names: Vec<String> = BUILTINS.iter().map(|b| b.name.to_owned()).collect();
    for key in file.environments.keys() {
        if !names.iter().any(|n| n == key) {
            names.push(key.clone());
        }
    }
    names.iter().map(|n| resolve_with(n, file, var)).collect()
}

fn is_known_with(
    name: &str,
    file: &EnvironmentsFile,
    var: impl Fn(&str) -> Option<String>,
) -> bool {
    builtin(name).is_some()
        || file.environments.contains_key(name)
        || var(&format!("{}_API_URL", env_prefix(name))).is_some()
}

/// Resolve an environment by name (built-ins → local config → env var).
pub fn resolve(name: &str) -> Result<ResolvedEnv, EnvError> {
    match load_file() {
        Ok(file) => resolve_with(name, &file, |k| std::env::var(k).ok()),
        // The local config is optional; a malformed/unreadable file must not
        // brick built-in or `<PREFIX>_API_URL`-defined envs. Retry against an
        // empty file, and only surface the load error if `name` actually needed
        // the file to resolve.
        Err(load_err) => {
            let empty = EnvironmentsFile::default();
            resolve_with(name, &empty, |k| std::env::var(k).ok()).map_err(|_| load_err)
        }
    }
}

/// The default environment's built-in API base URL.
///
/// Infallible last-resort value (unlike [`resolve`], which can fail on a
/// malformed local config), so callers never end up with an empty base URL.
pub fn default_api_url() -> &'static str {
    builtin(DEFAULT_ENV)
        .map(|b| b.api_url)
        .unwrap_or("https://api.godaddy.com")
}

/// Environments to show in `env list`: built-ins + local-config entries.
pub fn listable() -> Result<Vec<ResolvedEnv>, EnvError> {
    let file = load_file()?;
    listable_with(&file, |k| std::env::var(k).ok())
}

/// Whether `name` is a usable environment (built-in, locally configured, or
/// defined via a `<PREFIX>_API_URL` env var).
pub fn is_known(name: &str) -> bool {
    match load_file() {
        Ok(file) => is_known_with(name, &file, |k| std::env::var(k).ok()),
        // If the config can't be read, fall back to built-ins + env vars so a
        // broken file never hides the public environments.
        Err(_) => {
            builtin(name).is_some()
                || std::env::var(format!("{}_API_URL", env_prefix(name))).is_ok()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(api_url: &str) -> EnvEntry {
        EnvEntry {
            api_url: api_url.to_owned(),
            client_id: None,
            auth_url: None,
            token_url: None,
        }
    }

    fn no_vars(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn builtin_resolves_with_derived_oauth_urls() {
        let file = EnvironmentsFile::default();
        let env = resolve_with("prod", &file, no_vars).expect("prod resolves");
        assert_eq!(env.api_url, "https://api.godaddy.com");
        assert_eq!(env.client_id, "39489dee-4103-4284-9aab-9f2452142bce");
        assert_eq!(env.auth_url, "https://api.godaddy.com/v2/oauth2/authorize");
        assert_eq!(env.token_url, "https://api.godaddy.com/v2/oauth2/token");
    }

    #[test]
    fn unknown_env_errors_with_known_list() {
        let file = EnvironmentsFile::default();
        let err = resolve_with("nope", &file, no_vars).expect_err("unknown errors");
        let EnvError::Unknown { name, known } = err else {
            unreachable!("expected Unknown variant");
        };
        assert_eq!(name, "nope");
        assert!(known.contains("ote") && known.contains("prod"), "{known}");
    }

    #[test]
    fn env_var_defines_a_new_env() {
        let file = EnvironmentsFile::default();
        let var = |k: &str| (k == "DEV_API_URL").then(|| "https://dev.example.test".to_owned());
        let env = resolve_with("dev", &file, var).expect("dev resolves from env var");
        assert_eq!(env.api_url, "https://dev.example.test");
        assert_eq!(env.auth_url, "https://dev.example.test/v2/oauth2/authorize");
        assert!(env.client_id.is_empty());
    }

    #[test]
    fn env_var_overrides_a_builtin() {
        let file = EnvironmentsFile::default();
        let var =
            |k: &str| (k == "PROD_API_URL").then(|| "https://sandbox.example.test".to_owned());
        let env = resolve_with("prod", &file, var).expect("prod resolves");
        assert_eq!(env.api_url, "https://sandbox.example.test");
        // Client id is retained from the built-in.
        assert_eq!(env.client_id, "39489dee-4103-4284-9aab-9f2452142bce");
    }

    #[test]
    fn local_config_entry_resolves_and_respects_precedence() {
        let mut file = EnvironmentsFile::default();
        file.environments
            .insert("test".to_owned(), entry("https://test.example.invalid"));
        // No env var: local config wins.
        let env = resolve_with("test", &file, no_vars).expect("test resolves");
        assert_eq!(env.api_url, "https://test.example.invalid");
        // Env var present: overrides the local config api_url.
        let var =
            |k: &str| (k == "TEST_API_URL").then(|| "https://override.example.test".to_owned());
        let env = resolve_with("test", &file, var).expect("test resolves");
        assert_eq!(env.api_url, "https://override.example.test");
    }

    #[test]
    fn explicit_oauth_urls_and_client_id_in_local_config() {
        let mut file = EnvironmentsFile::default();
        file.environments.insert(
            "dev".to_owned(),
            EnvEntry {
                api_url: "https://dev.example.invalid".to_owned(),
                client_id: Some("custom-client".to_owned()),
                auth_url: Some("https://auth.example.invalid/authorize".to_owned()),
                token_url: Some("https://auth.example.invalid/token".to_owned()),
            },
        );
        let env = resolve_with("dev", &file, no_vars).expect("dev resolves");
        assert_eq!(env.client_id, "custom-client");
        assert_eq!(env.auth_url, "https://auth.example.invalid/authorize");
        assert_eq!(env.token_url, "https://auth.example.invalid/token");
    }

    #[test]
    fn oauth_urls_derive_from_custom_api_url_trimming_trailing_slash() {
        let mut file = EnvironmentsFile::default();
        // Custom env, no explicit auth/token URLs, api_url has a trailing slash.
        file.environments
            .insert("dev".to_owned(), entry("https://dev.example.invalid/"));
        let env = resolve_with("dev", &file, no_vars).expect("dev resolves");
        // api_url is normalized (trailing slash trimmed) so callers don't build `//`.
        assert_eq!(env.api_url, "https://dev.example.invalid");
        assert_eq!(
            env.auth_url,
            "https://dev.example.invalid/v2/oauth2/authorize"
        );
        assert_eq!(env.token_url, "https://dev.example.invalid/v2/oauth2/token");
    }

    #[test]
    fn default_api_url_is_the_builtin_prod_url() {
        assert_eq!(default_api_url(), "https://api.godaddy.com");
    }

    #[test]
    fn listable_includes_builtins_and_local_but_not_env_var_only() {
        let mut file = EnvironmentsFile::default();
        file.environments
            .insert("test".to_owned(), entry("https://test.example.invalid"));
        // `foo` is defined only via an env var and must NOT appear in the list.
        let var = |k: &str| (k == "FOO_API_URL").then(|| "https://foo.example.test".to_owned());
        let listed = listable_with(&file, var).expect("listable");
        let names: Vec<&str> = listed.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"ote"));
        assert!(names.contains(&"prod"));
        assert!(names.contains(&"test"));
        assert!(!names.contains(&"foo"), "env-var-only env leaked into list");
    }

    #[test]
    fn is_known_covers_builtin_local_and_env_var() {
        let mut file = EnvironmentsFile::default();
        file.environments
            .insert("test".to_owned(), entry("https://test.example.invalid"));
        let var = |k: &str| (k == "FOO_API_URL").then(|| "https://foo.example.test".to_owned());
        assert!(is_known_with("prod", &file, var)); // built-in
        assert!(is_known_with("test", &file, var)); // local config
        assert!(is_known_with("foo", &file, var)); // env var
        assert!(!is_known_with("missing", &file, var));
    }

    #[test]
    fn missing_config_file_is_not_an_error() {
        // load_file resolves a real path; just assert it does not panic and that
        // a default (empty) file resolves built-ins correctly via the public API.
        assert!(is_known("prod"));
    }
}
