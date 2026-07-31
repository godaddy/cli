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
    if probe.source(default_raw).is_ok() {
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
mod tests {
    use super::*;
    use cli_engine::environments::EnvTable;
    use std::sync::Mutex;

    // Serializes every test that touches real process env vars, so
    // parallel test threads can't observe each other's GDDY_* overrides.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// RAII guard that removes an env var on drop, even if a test panics.
    struct EnvGuard(&'static str);
    impl Drop for EnvGuard {
        fn drop(&mut self) {
            // SAFETY: caller holds ENV_LOCK; clean up on any exit incl. panic.
            #[allow(unsafe_code)]
            unsafe {
                std::env::remove_var(self.0)
            }
        }
    }

    #[test]
    fn register_scaffolds_a_file_only_environment() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("environments.toml");
        std::fs::write(
            &file,
            r#"
[dev]
api_url = "https://api.dev-godaddy.com"
client_id = "dev-client"
"#,
        )
        .expect("write file");

        let envs = register(Environments::new("prod").with_config_file_path_override(file));
        let resolved: GddyEnvConfig = envs.resolve("dev").expect("dev resolves");

        assert_eq!(resolved.domains_api_url, "https://api.dev-godaddy.com");
        assert_eq!(resolved.account_url, "https://account.dev-godaddy.com");
    }

    #[test]
    fn register_rejects_a_file_only_environments_malformed_api_url() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("environments.toml");
        std::fs::write(
            &file,
            r#"
[dev]
api_url = "not-a-url"
client_id = "dev-client"
"#,
        )
        .expect("write file");

        let envs = register(Environments::new("prod").with_config_file_path_override(file));
        let err = envs
            .resolve::<GddyEnvConfig>("dev")
            .expect_err("a malformed api_url must be a hard error, not silently dropped");
        assert!(err.to_string().contains("api_url"));
    }

    #[test]
    fn register_rejects_a_malformed_file_layer_auth_url_override_for_a_builtin() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("environments.toml");
        std::fs::write(
            &file,
            r#"
[prod]
auth_url = "not-a-url"
"#,
        )
        .expect("write file");

        let envs = register(Environments::new("prod").with_config_file_path_override(file));
        let err = envs
            .resolve::<GddyEnvConfig>("prod")
            .expect_err("a malformed override must be a hard error");
        assert!(err.to_string().contains("auth_url"));
    }

    fn test_environment(name: &str, extend: impl FnOnce(EnvTable) -> EnvTable) -> GddyEnvConfig {
        Environments::new(name)
            .with_environment(name, extend(EnvTable::new()))
            .resolve(name)
            .expect("resolves")
    }

    #[test]
    fn resolved_env_derives_oauth_urls_from_api_url_when_unset() {
        let resolved = test_environment("dev", |t| {
            t.with("client_id", "cid")
                .with("api_url", "https://api.example.test")
        });
        assert_eq!(resolved.name, "dev");
        assert_eq!(resolved.api_url, "https://api.example.test");
        assert_eq!(resolved.client_id, "cid");
        assert_eq!(
            resolved.auth_url,
            "https://api.example.test/v2/oauth2/authorize"
        );
        assert_eq!(
            resolved.token_url,
            "https://api.example.test/v2/oauth2/token"
        );
    }

    #[test]
    fn resolved_env_prefers_explicit_oauth_urls_over_derived() {
        let resolved = test_environment("dev", |t| {
            t.with("client_id", "cid")
                .with("auth_url", "https://auth.example.test/authorize")
                .with("token_url", "https://auth.example.test/token")
                .with("api_url", "https://api.example.test")
        });
        assert_eq!(resolved.auth_url, "https://auth.example.test/authorize");
        assert_eq!(resolved.token_url, "https://auth.example.test/token");
    }

    #[test]
    fn resolved_env_falls_back_to_derived_oauth_urls_when_override_is_blank() {
        // A blank `auth_url` (from a TOML value here, or from `GDDY_AUTH_URL=" "`
        // — see `blank_env_var_override_falls_through_to_derived_auth_url`) is
        // treated the same as unset. A genuinely malformed (non-blank) override
        // is a hard resolve error instead — see
        // `register_rejects_a_malformed_file_layer_auth_url_override_for_a_builtin`.
        let resolved = test_environment("dev", |t| {
            t.with("client_id", "cid")
                .with("auth_url", "   ")
                .with("api_url", "https://api.example.test")
        });
        assert_eq!(
            resolved.auth_url,
            "https://api.example.test/v2/oauth2/authorize"
        );
    }

    #[test]
    fn resolve_rejects_missing_api_url() {
        let err = Environments::new("dev")
            .with_environment("dev", EnvTable::new().with("client_id", "cid"))
            .resolve::<GddyEnvConfig>("dev")
            .expect_err("no api_url");
        assert!(err.to_string().contains("api_url"));
    }

    #[test]
    fn resolve_rejects_missing_client_id() {
        // client_id has no default: every environment (built-in, file, or
        // hand-built for a test) must supply a real one.
        let err = Environments::new("dev")
            .with_environment(
                "dev",
                EnvTable::new().with("api_url", "https://api.example.test"),
            )
            .resolve::<GddyEnvConfig>("dev")
            .expect_err("no client_id");
        assert!(err.to_string().contains("client_id"));
    }

    #[test]
    fn resolve_rejects_blank_api_url_final_value() {
        // Unlike auth_url/token_url/domains_api_url/account_url, api_url has
        // no sensible derived fallback, so blank is rejected (as `MissingField`,
        // since a blank source answer is treated as absent by default).
        let err = Environments::new("dev")
            .with_environment(
                "dev",
                EnvTable::new()
                    .with("client_id", "cid")
                    .with("api_url", "   "),
            )
            .resolve::<GddyEnvConfig>("dev")
            .expect_err("blank api_url must be rejected");
        assert!(err.to_string().contains("api_url"));
    }

    #[test]
    fn domains_api_url_defaults_to_api_url() {
        let resolved = test_environment("dev", |t| {
            t.with("client_id", "cid")
                .with("api_url", "https://api.example.test")
        });
        assert_eq!(resolved.domains_api_url, resolved.api_url);
    }

    #[test]
    fn domains_api_url_override_is_respected() {
        let resolved = test_environment("dev", |t| {
            t.with("client_id", "cid")
                .with("api_url", "https://api.example.test")
                .with("domains_api_url", "https://domains.example.test")
        });
        assert_eq!(resolved.domains_api_url, "https://domains.example.test");
    }

    #[test]
    fn account_url_defaults_to_bare_domain_for_prod() {
        let resolved = test_environment("prod", |t| {
            t.with("client_id", "cid")
                .with("api_url", "https://api.godaddy.com")
        });
        assert_eq!(resolved.account_url, "https://account.godaddy.com");
    }

    #[test]
    fn account_url_defaults_to_prefixed_domain_for_non_prod() {
        let resolved = test_environment("ote", |t| {
            t.with("client_id", "cid")
                .with("api_url", "https://api.ote-godaddy.com")
        });
        assert_eq!(resolved.account_url, "https://account.ote-godaddy.com");
    }

    #[test]
    fn account_url_override_is_respected() {
        let resolved = test_environment("dev", |t| {
            t.with("client_id", "cid")
                .with("api_url", "https://api.example.test")
                .with("account_url", "https://account.override.test")
        });
        assert_eq!(resolved.account_url, "https://account.override.test");
    }

    fn test_environment_with_app_id(
        name: &str,
        extend: impl FnOnce(EnvTable) -> EnvTable,
    ) -> GddyEnvConfig {
        Environments::new(name)
            .with_app_id(APP_ID)
            .with_environment(name, extend(EnvTable::new()))
            .resolve(name)
            .expect("resolves")
    }

    #[test]
    fn env_var_overrides_auth_url() {
        let _g = ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // SAFETY: serialized by ENV_LOCK; guard removes the var on any exit.
        #[allow(unsafe_code)]
        unsafe {
            std::env::set_var("GDDY_AUTH_URL", "https://auth.override.test")
        };
        let _guard = EnvGuard("GDDY_AUTH_URL");

        let resolved = test_environment_with_app_id("dev", |t| {
            t.with("client_id", "cid")
                .with("api_url", "https://api.example.test")
                .with("auth_url", "https://auth.example.test/authorize")
        });
        assert_eq!(
            resolved.auth_url, "https://auth.override.test",
            "env var must win over the TOML value"
        );
    }

    #[test]
    fn env_var_overrides_token_url() {
        let _g = ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // SAFETY: serialized by ENV_LOCK; guard removes the var on any exit.
        #[allow(unsafe_code)]
        unsafe {
            std::env::set_var("GDDY_TOKEN_URL", "https://token.override.test")
        };
        let _guard = EnvGuard("GDDY_TOKEN_URL");

        let resolved = test_environment_with_app_id("dev", |t| {
            t.with("client_id", "cid")
                .with("api_url", "https://api.example.test")
        });
        assert_eq!(resolved.token_url, "https://token.override.test");
    }

    #[test]
    fn env_var_overrides_domains_api_url() {
        let _g = ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // SAFETY: serialized by ENV_LOCK; guard removes the var on any exit.
        #[allow(unsafe_code)]
        unsafe {
            std::env::set_var("GDDY_DOMAINS_API_URL", "https://domains.override.test")
        };
        let _guard = EnvGuard("GDDY_DOMAINS_API_URL");

        let resolved = test_environment_with_app_id("dev", |t| {
            t.with("client_id", "cid")
                .with("api_url", "https://api.example.test")
        });
        assert_eq!(resolved.domains_api_url, "https://domains.override.test");
    }

    #[test]
    fn env_var_overrides_account_url() {
        let _g = ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // SAFETY: serialized by ENV_LOCK; guard removes the var on any exit.
        #[allow(unsafe_code)]
        unsafe {
            std::env::set_var("GDDY_ACCOUNT_URL", "https://account.override.test")
        };
        let _guard = EnvGuard("GDDY_ACCOUNT_URL");

        let resolved = test_environment_with_app_id("dev", |t| {
            t.with("client_id", "cid")
                .with("api_url", "https://api.example.test")
        });
        assert_eq!(resolved.account_url, "https://account.override.test");
    }

    #[test]
    fn substitute_env_host_prefixes_a_bare_domain() {
        assert_eq!(
            substitute_env_host("https://godaddy.com", "ote"),
            Some("https://ote-godaddy.com".to_owned())
        );
    }

    #[test]
    fn substitute_env_host_preserves_subdomain_and_path() {
        assert_eq!(
            substitute_env_host(
                "https://fulfillment.api.commerce.godaddy.com/v1/commerce",
                "dev"
            ),
            Some("https://fulfillment.api.commerce.dev-godaddy.com/v1/commerce".to_owned())
        );
    }

    #[test]
    fn substitute_env_host_returns_none_for_non_godaddy_host() {
        assert_eq!(substitute_env_host("https://example.com/v1", "ote"), None);
    }

    #[test]
    fn domain_override_falls_back_to_env_var_when_no_local_config_entry() {
        // "fulfillments-catalog-test" is not a compiled builtin and has no
        // local config entry, so the first (local-config) branch misses and
        // the injected var getter is consulted directly.
        let var = |k: &str| {
            (k == "FULFILLMENTS_CATALOG_TEST_FULFILLMENTS_API_URL")
                .then(|| "https://fulfillments.example.test".to_owned())
        };
        let resolved = domain_override("fulfillments_api_url", "fulfillments-catalog-test", var);
        assert_eq!(
            resolved,
            Some("https://fulfillments.example.test".to_owned())
        );
    }

    #[test]
    fn domain_override_is_none_when_neither_layer_has_it() {
        let resolved = domain_override("fulfillments_api_url", "fulfillments-catalog-test", |_| {
            None
        });
        assert_eq!(resolved, None);
    }

    #[test]
    fn resolve_catalog_base_url_returns_prod_unchanged() {
        let url = resolve_catalog_base_url(
            "fulfillments",
            "https://fulfillment.api.commerce.godaddy.com/v1/commerce",
            "prod",
        );
        assert_eq!(
            url,
            "https://fulfillment.api.commerce.godaddy.com/v1/commerce"
        );
    }

    #[test]
    fn resolve_catalog_base_url_applies_convention_for_non_prod() {
        // No override exists anywhere for this made-up env/domain pair, so
        // this exercises the `{env}-godaddy.com` convention fallback.
        let url = resolve_catalog_base_url(
            "fulfillments",
            "https://fulfillment.api.commerce.godaddy.com/v1/commerce",
            "ote",
        );
        assert_eq!(
            url,
            "https://fulfillment.api.commerce.ote-godaddy.com/v1/commerce"
        );
    }

    #[test]
    fn env_var_override_rejects_a_malformed_url() {
        let _g = ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // SAFETY: serialized by ENV_LOCK; guard removes the var on any exit.
        #[allow(unsafe_code)]
        unsafe {
            std::env::set_var("GDDY_AUTH_URL", "not-a-url")
        };
        let _guard = EnvGuard("GDDY_AUTH_URL");

        let err = Environments::new("dev")
            .with_app_id(APP_ID)
            .with_environment(
                "dev",
                EnvTable::new()
                    .with("client_id", "cid")
                    .with("api_url", "https://api.example.test"),
            )
            .resolve::<GddyEnvConfig>("dev")
            .expect_err("a malformed env var override must be a hard error");
        assert!(err.to_string().contains("auth_url"));
    }

    #[test]
    fn blank_env_var_override_falls_through_to_derived_auth_url() {
        let _g = ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // SAFETY: serialized by ENV_LOCK; guard removes the var on any exit.
        #[allow(unsafe_code)]
        unsafe {
            std::env::set_var("GDDY_AUTH_URL", "   ")
        };
        let _guard = EnvGuard("GDDY_AUTH_URL");

        let resolved = test_environment_with_app_id("dev", |t| {
            t.with("client_id", "cid")
                .with("api_url", "https://api.example.test")
        });
        assert_eq!(
            resolved.auth_url, "https://api.example.test/v2/oauth2/authorize",
            "a blank env var override is treated as absent, same as a blank TOML value"
        );
    }

    #[test]
    fn clean_url_requires_a_non_empty_host() {
        assert_eq!(
            clean_url("https://api.example.test/"),
            Some("https://api.example.test".to_owned())
        );
        assert!(clean_url("https:///path").is_none());
        assert!(clean_url("https://").is_none());
        assert!(clean_url("https://?x").is_none());
        assert!(clean_url("ftp://x").is_none());
        assert!(clean_url("api.example.test").is_none());
        assert!(clean_url("not a url").is_none());
        assert_eq!(
            clean_url("HTTPS://api.Example.test"),
            Some("HTTPS://api.Example.test".to_owned())
        );
        assert_eq!(
            clean_url("http://localhost:8080/api/"),
            Some("http://localhost:8080/api".to_owned())
        );
    }

    #[test]
    fn env_prefix_uppercases_and_replaces_hyphen() {
        assert_eq!(env_prefix("ote"), "OTE");
        assert_eq!(env_prefix("prod-us"), "PROD_US");
    }

    #[test]
    fn resolve_default_environments_falls_back_to_default_env_for_an_unresolvable_gdenv_value() {
        // A corrupted/stale `.gdenv` value that resolves to nothing (no
        // compiled/file entry) must not become the CLI's real startup
        // default.
        let envs = resolve_default_environments("totally-bogus-env-name");
        assert_eq!(envs.default_env(), DEFAULT_ENV);
        assert!(envs.source(DEFAULT_ENV).is_ok());
    }

    #[test]
    fn resolve_default_environments_keeps_a_resolvable_gdenv_value() {
        let envs = resolve_default_environments("prod");
        assert_eq!(envs.default_env(), "prod");
    }

    #[test]
    fn devx_core_url_uses_prod_and_ote_builtins() {
        assert_eq!(
            devx_core_url_with("prod", |_| None).as_deref(),
            Some("https://api.developer.commerce.godaddy.com")
        );
        assert_eq!(
            devx_core_url_with("ote", |_| None).as_deref(),
            Some("https://api.developer.commerce.ote-godaddy.com")
        );
    }

    #[test]
    fn devx_core_url_global_override_wins() {
        assert_eq!(
            devx_core_url_with("prod", |key| {
                (key == "DEVX_CORE_URL").then(|| " http://localhost:4000/ ".to_owned())
            })
            .as_deref(),
            Some("http://localhost:4000")
        );
    }

    #[test]
    fn devx_core_url_per_environment_override_wins_over_global() {
        assert_eq!(
            devx_core_url_with("dev", |key| match key {
                "DEV_DEVX_CORE_URL" => Some("https://dev-core.example.test/".to_owned()),
                "DEVX_CORE_URL" => Some("https://shared-core.example.test".to_owned()),
                _ => None,
            })
            .as_deref(),
            Some("https://dev-core.example.test")
        );
    }

    #[test]
    fn devx_core_url_custom_env_requires_override() {
        assert_eq!(devx_core_url_with("dev", |_| None), None);
    }
}
