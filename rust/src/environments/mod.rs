//! Environment -> endpoint resolution, delegated to cli-engine's shared
//! `Environments`/`EnvConfig` mechanism.
//!
//! Built-in, public-safe environments (`ote`, `prod`) are compiled in here.
//! Internal DEV/TEST environments are supplied **at runtime** and never
//! committed to this (OSS) repo, via a `gddy/environments.toml` in the OS
//! config directory.
//!
//! # Feature-flag visibility per environment
//!
//! gddy's global default (set on `CliConfig` in `main.rs`) keeps
//! `min_stage` at `Stage::Ga`, hiding any module/command flagged below GA
//! (e.g. `hosting` = Beta; `platform` = Experimental).
//! An `environments.toml` entry can permanently opt a specific environment
//! into those pre-release commands via the *same* recognized keys cli-engine
//! reads generically off every environment's merged TOML table:
//!
//! ```toml
//! [dev]
//! api_url = "https://api.dev-godaddy.com"
//! min_stage = "experimental"
//!
//! [staging.feature_overrides]
//! "some-flag-key" = "beta"
//! ```

use std::sync::{Arc, LazyLock, OnceLock};

use cli_engine::environments::Environments;
use cli_engine::{ConfigSource, EnvConfig, SourceChain};

pub const DEFAULT_ENV: &str = "prod";

/// The two fields a compiled-in environment actually sets.
#[derive(Debug, Clone, EnvConfig)]
struct BaseEnvConfig {
    api_url: String,
    client_id: String,
}

/// The compiled-in `ote`/`prod` environments.
static BUILTIN_ENVS: LazyLock<Vec<(&'static str, BaseEnvConfig)>> = LazyLock::new(|| {
    vec![
        (
            "ote",
            BaseEnvConfig {
                api_url: "https://api.ote-godaddy.com".to_owned(),
                client_id: "91660d79-c909-426c-b5c8-e0f575e8fcd2".to_owned(),
            },
        ),
        (
            "prod",
            BaseEnvConfig {
                api_url: "https://api.godaddy.com".to_owned(),
                client_id: "bc87f347-af82-4892-833f-818f54a0e79e".to_owned(),
            },
        ),
    ]
});

/// Scopes requested at login by default.
pub const DEFAULT_OAUTH_SCOPES: &[&str] = &[
    crate::scopes::APP_REGISTRY_READ,
    crate::scopes::DOMAINS_READ,
    crate::scopes::OFFLINE_ACCESS,
];
pub const REDIRECT_URI: &str = "http://localhost:7443/callback";
pub const APP_ID: &str = "gddy";

/// DevX Core API gateway base URL for each compiled-in builtin, consulted by
/// [`devx_core_url_with`] only after both env-var override tiers miss.
const BUILTIN_DEVX_CORE_URLS: &[(&str, &str)] = &[
    ("ote", "https://api.developer.commerce.ote-godaddy.com"),
    ("prod", "https://api.developer.commerce.godaddy.com"),
];

/// A fully-resolved environment config
#[derive(Debug, Clone, Default, EnvConfig)]
pub struct GddyEnvConfig {
    #[env_config(default_fn = default_name)]
    pub name: String,

    #[env_config(from_toml = parse_url_from_toml)]
    pub api_url: String,

    /// No default: every environment must supply a real OAuth client id.
    pub client_id: String,

    /// Overridable at runtime via `GDDY_AUTH_URL` — e.g. to point at a local
    /// dev auth server without editing `environments.toml`.
    #[env_config(
        from_toml = parse_url_from_toml,
        env = "AUTH_URL",
        from_env = parse_url,
        default_fn = default_auth_url
    )]
    pub auth_url: String,

    /// Overridable at runtime via `GDDY_TOKEN_URL`.
    #[env_config(
        from_toml = parse_url_from_toml,
        env = "TOKEN_URL",
        from_env = parse_url,
        default_fn = default_token_url
    )]
    pub token_url: String,

    /// Base URL for the domain commands. Some endpoints (e.g. domain
    /// availability) live behind a different host than the OAuth/`api_url`
    /// service; this defaults to `api_url` when not overridden. Overridable
    /// at runtime via `GDDY_DOMAINS_API_URL`.
    #[env_config(
        from_toml = parse_url_from_toml,
        env = "DOMAINS_API_URL",
        from_env = parse_url,
        default_fn = default_domains_api_url
    )]
    pub domains_api_url: String,

    /// Base URL for the account management site (e.g. adding payment methods).
    /// Defaults to `account.godaddy.com` for prod and `account.{env}-godaddy.com`
    /// for other environments; overridable via `GDDY_ACCOUNT_URL` or local config.
    #[env_config(
        from_toml = parse_url_from_toml,
        env = "ACCOUNT_URL",
        from_env = parse_url,
        default_fn = default_account_url
    )]
    pub account_url: String,

    /// Base URL for the email (panel-v3) API. Defaults to
    /// `productivity.api.godaddy.com` for prod, `productivity.api.test-godaddy.com`
    /// for test, and `productivity.api.stg-godaddy.com` for stage — not the
    /// generic `{env}-godaddy.com` convention, since `stage`'s real host uses
    /// `stg-` and there's no dedicated OTE deployment (`ote` aliases to
    /// `test`). Any other environment falls back to `api_url`. Overridable at
    /// runtime via `GDDY_EMAIL_API_URL` or local config.
    #[env_config(
        from_toml = parse_url_from_toml,
        env = "EMAIL_API_URL",
        from_env = parse_url,
        default_fn = default_email_api_url
    )]
    pub email_api_url: String,
}

pub fn env_prefix(name: &str) -> String {
    name.to_uppercase().replace('-', "_")
}

/// `name`'s `default_fn`: the field itself is never set by any real TOML/env
/// source, so this fires unconditionally, reading the environment's own
/// identity off the chain instead of a sibling field's value.
fn default_name(sources: &SourceChain<'_>) -> String {
    sources.env_name().unwrap_or_default().to_owned()
}

/// The already-resolved, cleaned `api_url` for this chain — same value the
/// `api_url` field itself holds, re-derived from the raw chain rather than
/// `Self` (a `default_fn` only ever sees the chain, not sibling fields
/// already computed on the struct being built). Declaring `api_url` before
/// the fields that call this, so their own resolution only runs once
/// `api_url`'s already succeeded, is what makes re-deriving from the raw
/// value safe — a malformed `api_url` fails the whole `assemble` before any
/// of these `default_fn`s could run.
fn current_api_url(sources: &SourceChain<'_>) -> String {
    sources
        .toml_value("api_url")
        .and_then(toml::Value::as_str)
        .and_then(clean_url)
        .unwrap_or_default()
}

fn default_auth_url(sources: &SourceChain<'_>) -> String {
    derive_auth_url(&current_api_url(sources))
}

fn default_token_url(sources: &SourceChain<'_>) -> String {
    derive_token_url(&current_api_url(sources))
}

fn default_domains_api_url(sources: &SourceChain<'_>) -> String {
    current_api_url(sources)
}

fn default_account_url(sources: &SourceChain<'_>) -> String {
    derive_account_url(sources.env_name().unwrap_or_default())
}

/// Compiled-in per-environment hosts for the panel-v3 email API — see the
/// doc comment on [`GddyEnvConfig::email_api_url`] for why this can't be
/// derived from `api_url` via the generic host-substitution convention.
const BUILTIN_EMAIL_API_URLS: &[(&str, &str)] = &[
    ("prod", "https://productivity.api.godaddy.com"),
    ("test", "https://productivity.api.test-godaddy.com"),
    ("stage", "https://productivity.api.stg-godaddy.com"),
    // No dedicated OTE deployment of this API; alias to `test`.
    ("ote", "https://productivity.api.test-godaddy.com"),
];

fn default_email_api_url(sources: &SourceChain<'_>) -> String {
    let env_name = sources.env_name().unwrap_or_default();
    BUILTIN_EMAIL_API_URLS
        .iter()
        .find(|(name, _)| *name == env_name)
        .map(|(_, url)| (*url).to_owned())
        .unwrap_or_else(|| current_api_url(sources))
}

fn derive_account_url(env_name: &str) -> String {
    if env_name == "prod" {
        return "https://account.godaddy.com".to_owned();
    }
    substitute_env_host("https://account.godaddy.com", env_name)
        .unwrap_or_else(|| "https://account.godaddy.com".to_owned())
}

/// Applies GoDaddy's internal-environment hostname convention
/// (`{env}-godaddy.com`) to a canonical `*.godaddy.com` URL, preserving any
/// subdomain prefix and the original scheme/path. Returns `None` if the
/// host isn't under `godaddy.com` — nothing to substitute.
fn substitute_env_host(url: &str, env_name: &str) -> Option<String> {
    let lower = url.to_ascii_lowercase();
    let scheme_len = if lower.starts_with("https://") {
        8
    } else if lower.starts_with("http://") {
        7
    } else {
        return None;
    };
    let rest = &url[scheme_len..];
    let host_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let host = &rest[..host_end];
    let host_lower = host.to_ascii_lowercase();
    let new_host = if host_lower == "godaddy.com" {
        format!("{env_name}-godaddy.com")
    } else {
        let prefix = host_lower.strip_suffix(".godaddy.com")?;
        format!("{}.{env_name}-godaddy.com", &host[..prefix.len()])
    };
    Some(format!(
        "{}{}{}",
        &url[..scheme_len],
        new_host,
        &rest[host_end..]
    ))
}

/// Resolve the environment-specific base URL for a catalog domain (used by
/// `api domain list` / `api call`). Checks an explicit override first — a
/// `<domain>_api_url` key from local config, or a `<PREFIX>_<DOMAIN>_API_URL`
/// env var read directly here (outside `GddyEnvConfig`'s own fields entirely,
/// since a per-domain override key isn't one of them). Checking the env var
/// directly makes this work uniformly for `prod`/`ote` builtins and custom
/// dev/test env names alike. Absent an override, falls back to GoDaddy's
/// `{env}-godaddy.com` hostname convention against `prod_base_url` for any
/// non-`prod` environment.
pub fn resolve_catalog_base_url(domain: &str, prod_base_url: &str, env_name: &str) -> String {
    let override_key = format!("{}_api_url", domain.replace('-', "_"));
    if let Some(url) = domain_override(&override_key, env_name, |k| std::env::var(k).ok()) {
        return url;
    }
    if env_name == DEFAULT_ENV {
        return prod_base_url.to_owned();
    }
    substitute_env_host(prod_base_url, env_name).unwrap_or_else(|| prod_base_url.to_owned())
}

/// Pure lookup for [`resolve_catalog_base_url`]'s override, with the env-var
/// getter injected so tests stay parallel-safe (no real `std::env::set_var`).
/// `instance().source(env_name)` errors for an environment unknown to every
/// compiled/file layer, which this treats the same as "no local config
/// entry" — falling through to the env var — rather than propagating.
fn domain_override(
    override_key: &str,
    env_name: &str,
    var: impl Fn(&str) -> Option<String>,
) -> Option<String> {
    if let Ok(source) = instance().source(env_name)
        && let Some(raw) = source
            .toml_value(override_key)
            .and_then(toml::Value::as_str)
        && let Some(clean) = clean_url(raw)
    {
        return Some(clean);
    }
    let var_name = format!("{}_{}", env_prefix(env_name), override_key.to_uppercase());
    var(&var_name).and_then(|v| clean_url(&v))
}

fn derive_auth_url(api_url: &str) -> String {
    format!("{}/v2/oauth2/authorize", api_url.trim_end_matches('/'))
}

fn derive_token_url(api_url: &str) -> String {
    format!("{}/v2/oauth2/token", api_url.trim_end_matches('/'))
}

/// Validates and normalizes a candidate URL. Trims surrounding
/// whitespace and any trailing slash, and requires an `http(s)://` scheme with a
/// non-empty host (reqwest needs an absolute URL). Returns `None` for an
/// empty/whitespace or schemeless value, so a blank or malformed value never
/// resolves to a relative/unusable URL.
fn clean_url(raw: &str) -> Option<String> {
    let trimmed = raw.trim().trim_end_matches('/');
    // Require an http(s):// scheme (case-insensitive per RFC 3986, so `HTTPS://`
    // is valid) and a non-empty host. The host is the segment before any
    // path/query/fragment, so this rejects `https:///path`, `https://`, and
    // `https://?x` (which a lenient URL parser would accept).
    let lower = trimmed.to_ascii_lowercase();
    let scheme_len = if lower.starts_with("https://") {
        "https://".len()
    } else if lower.starts_with("http://") {
        "http://".len()
    } else {
        return None;
    };
    let host = trimmed[scheme_len..]
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("");
    (!host.is_empty()).then(|| trimmed.to_owned())
}

/// Base URL for the DevX Core API gateway for the given environment.
///
/// Custom environments must set `<PREFIX>_DEVX_CORE_URL` (for example,
/// `DEV_DEVX_CORE_URL`) or the global `DEVX_CORE_URL`. `prod` and `ote` use
/// their compiled-in endpoints unless either variable overrides them.
pub fn devx_core_url(name: &str) -> Option<String> {
    devx_core_url_with(name, |key| std::env::var(key).ok())
}

fn devx_core_url_with(name: &str, var: impl Fn(&str) -> Option<String>) -> Option<String> {
    let prefix = env_prefix(name);
    var(&format!("{prefix}_DEVX_CORE_URL"))
        .and_then(|value| clean_url(&value))
        .or_else(|| var("DEVX_CORE_URL").and_then(|value| clean_url(&value)))
        .or_else(|| {
            BUILTIN_DEVX_CORE_URLS
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, url)| (*url).to_owned())
        })
}

/// Validates a candidate URL string. `EnvConfig` `from_env` shared by every
/// env-var-overridable URL field — an env var is already a plain `&str`, so
/// this is the direct validator with nothing to unwrap first. Blank values
/// never reach this — every `EnvConfig` field treats a blank source answer
/// as absent by default; a non-blank value must be a real http(s) URL.
fn parse_url(raw: &str) -> Result<String, String> {
    clean_url(raw).ok_or_else(|| format!("{raw:?} is not a valid http(s) URL"))
}

/// `EnvConfig` `from_toml` shared by every URL field — a TOML value carries
/// its own type, so this checks it's actually a string before delegating to
/// [`parse_url`], the shared core every URL field's `from_env` also uses
/// directly.
fn parse_url_from_toml(value: &toml::Value) -> Result<String, String> {
    let raw = value
        .as_str()
        .ok_or_else(|| "expected a string".to_owned())?;
    parse_url(raw)
}

/// Builds the compiled-in `ote`/`prod` `Environments`, shared by
/// `CliConfig::with_environments`, `PkceAuthProvider::with_environments`, and
/// this module's own resolution helpers below.
fn build_environments() -> Environments {
    // `crate::env::read_gdenv_raw`, not `crate::env::get_env` — `get_env`
    // validates via `is_known`, which resolves against this very instance and
    // would deadlock re-entering `instance()`'s `OnceLock`
    // mid-initialization.
    let default_raw = crate::env::read_gdenv_raw().unwrap_or_else(|| DEFAULT_ENV.to_owned());
    resolve_default_environments(&default_raw)
}

fn resolve_default_environments(default_raw: &str) -> Environments {
    let probe = register(Environments::new(default_raw.to_owned()));
    // `.resolve::<GddyEnvConfig>(...)`, not `.source(...)` — a name can be
    // *known* to some layer (so `.source()` succeeds) while still being
    // *unusable* (missing client_id, a malformed api_url, ...). Only a full
    // resolve proves the persisted default is actually safe to keep.
    if probe.resolve::<GddyEnvConfig>(default_raw).is_ok() {
        probe
    } else {
        register(Environments::new(DEFAULT_ENV))
    }
}

/// Registers the compiled `ote`/`prod` defaults onto `envs`
fn register(envs: Environments) -> Environments {
    let mut envs = envs.with_app_id(APP_ID).with_config_file(true);
    for (name, config) in BUILTIN_ENVS.iter() {
        envs = envs.with_environment(*name, config.clone());
    }
    envs
}

/// The shared `Environments` instance — built once, reused by every consumer
pub fn instance() -> &'static Arc<Environments> {
    static INSTANCE: OnceLock<Arc<Environments>> = OnceLock::new();
    INSTANCE.get_or_init(|| Arc::new(build_environments()))
}

/// Resolve an environment by name.
pub fn resolve(name: &str) -> cli_engine::Result<GddyEnvConfig> {
    // `?`, not a manual `CliCoreError::message` rewrap — `EnvConfigError`
    // already converts into `CliCoreError` (see cli-engine's `error.rs`),
    // preserving whatever structured system/fix metadata the underlying
    // error carries instead of flattening it to a plain string.
    Ok(instance().resolve(name)?)
}

/// Environments to show in `env list`: built-ins + local-config entries.
pub fn listable() -> cli_engine::Result<Vec<GddyEnvConfig>> {
    let envs = instance();
    Ok(envs
        .list()
        .into_iter()
        .filter_map(|name| envs.resolve::<GddyEnvConfig>(&name).ok())
        .collect())
}

/// Whether `name` is a usable environment (built-in or locally configured).
pub fn is_known(name: &str) -> bool {
    resolve(name).is_ok()
}

#[cfg(test)]
mod tests;
