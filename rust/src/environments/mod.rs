//! Environment -> endpoint resolution, delegated to cli-engine's shared
//! `Environments`/`EnvironmentDef` type.
//!
//! Built-in, public-safe environments (`ote`, `prod`) are compiled in here.
//! Internal DEV/TEST environments are supplied **at runtime** and never
//! committed to this (OSS) repo, via two override mechanisms:
//!
//! * **Per-env environment variable** — `<PREFIX>_API_URL` overrides (or, for a
//!   name unknown to every other layer, *defines*) an environment's API base
//!   URL, where `<PREFIX>` is the env name uppercased with `-` replaced by `_`
//!   (e.g. `DEV_API_URL`). cli-engine's `Environments` only lets an env var
//!   override a *field* of an already-known environment, not introduce a
//!   brand new selectable one, so [`resolve`] falls back to a local,
//!   env-var-only path for that case — internal hostnames still never need a
//!   compiled or local config entry.
//! * **Gitignored local config** — a `gddy/environments.toml` in the OS config
//!   directory, parsed by cli-engine's `Environments` file layer (which
//!   accepts both a flat `[name]` table and gddy's already-shipped nested
//!   `[environments.name]` table).
//!
//! Resolution order (later layers win): built-in base → local config entry →
//! `<PREFIX>_*` env var. Unlike cli-engine's own layering (a later layer's
//! blank/malformed value unconditionally overwrites a good earlier one), the
//! final merged URL strings are still run through [`clean_url`] here, so a
//! blank/malformed override is rejected rather than silently used.
//!
//! # Feature-flag visibility per environment
//!
//! gddy's global default (set on `CliConfig` in `main.rs`) keeps
//! `min_stage` at `Stage::Ga`, hiding any module/command flagged below GA
//! (e.g. `hosting` = Beta; `webhook`/`application`/`actions` = Experimental).
//! An `environments.toml` entry can permanently opt a specific environment
//! into those pre-release commands via the *same* recognized keys cli-engine
//! parses on every `EnvironmentDef` — no gddy-specific plumbing needed:
//!
//! ```toml
//! [environments.dev]
//! api_url = "https://api.dev-godaddy.com"
//! min_stage = "experimental"
//!
//! [environments.staging.feature_overrides]
//! "some-flag-key" = "beta"
//! ```
//!
//! `<ENV>_MIN_STAGE`/`<ENV>_FEATURE_<KEY>` env vars override the same fields.
//! **Caveat:** this only takes effect for the environment active when the
//! process *starts* — cli-engine computes the visible command tree once at
//! `Cli::new`, before the global `--env` flag is applied, so a same-invocation
//! `--env dev` does not reveal `dev`'s pre-release commands. Persist the
//! environment first (`gddy env set dev`), then run the command.

use std::sync::{Arc, OnceLock};

use cli_engine::{CliCoreError, Environment, EnvironmentDef, Environments};

pub const DEFAULT_ENV: &str = "prod";
/// Scopes requested at login by default. The authorization server may grant a
/// subset; commands needing more declare them and the provider steps up. Drawn
/// from the central [`crate::scopes`] registry (which the OAuth client mirrors).
pub const DEFAULT_OAUTH_SCOPES: &[&str] = &[
    crate::scopes::APP_REGISTRY_READ,
    crate::scopes::DOMAINS_READ,
    crate::scopes::OFFLINE_ACCESS,
];
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
        client_id: "91660d79-c909-426c-b5c8-e0f575e8fcd2",
    },
    Builtin {
        name: "prod",
        api_url: "https://api.godaddy.com",
        client_id: "bc87f347-af82-4892-833f-818f54a0e79e",
    },
];

/// A fully-resolved environment: everything needed to talk to it.
#[derive(Clone, Debug)]
pub struct ResolvedEnv {
    pub name: String,
    pub api_url: String,
    pub client_id: String,
    pub auth_url: String,
    pub token_url: String,
    /// Base URL for the domain commands. Some endpoints (e.g. domain
    /// availability) live behind a different host than the OAuth/`api_url`
    /// service; this defaults to `api_url` when not overridden.
    pub domains_api_url: String,
    /// Base URL for the account management site (e.g. adding payment methods).
    /// Defaults to `account.godaddy.com` for prod and `account.{env}-godaddy.com`
    /// for other environments; overridable via `<PREFIX>_ACCOUNT_URL` or local config.
    pub account_url: String,
}

/// The domain-command view of a resolved environment.
#[derive(Clone, Debug)]
pub struct ResolvedDomains {
    pub base_url: String,
}

pub fn env_prefix(name: &str) -> String {
    name.to_uppercase().replace('-', "_")
}

fn derive_account_url(env_name: &str) -> String {
    let host = if env_name == "prod" {
        "account.godaddy.com".to_owned()
    } else {
        format!("account.{env_name}-godaddy.com")
    };
    format!("https://{host}")
}

fn derive_auth_url(api_url: &str) -> String {
    format!("{}/v2/oauth2/authorize", api_url.trim_end_matches('/'))
}

fn derive_token_url(api_url: &str) -> String {
    format!("{}/v2/oauth2/token", api_url.trim_end_matches('/'))
}

/// Validates and normalizes a candidate URL (api/auth/token): trims surrounding
/// whitespace and any trailing slash, and requires an `http(s)://` scheme with a
/// non-empty host (reqwest needs an absolute URL). Returns `None` for an
/// empty/whitespace or schemeless value, so a blank or malformed value never
/// resolves to a relative/unusable URL. This is the single authority for "is
/// this URL usable?" across [`adapt`] and [`resolve_from_env_var_only`] —
/// unlike cli-engine's own file/env-var layers (which apply unconditionally,
/// with no such check), a blank/malformed value here is treated as unset.
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

/// Scans the raw process argv for a `--env <value>` / `--env=<value>` token.
///
/// cli-engine's built-in global `--env` flag (and any command-local `--env`
/// arg sharing that argv text, e.g. `pat add --env <name>`) validates against
/// the shared `Environments` *directly* — bypassing this module's own
/// [`resolve`] and its env-var-only fallback entirely. So a name defined only
/// via `<PREFIX>_API_URL` must be pre-registered into the singleton itself
/// (see [`build_environments`]) before cli-engine ever sees it, which means
/// finding that name before clap has parsed anything.
fn requested_env_from_argv() -> Option<String> {
    // `args_os()` + a lossy conversion, not `args()` — this runs during the
    // environment singleton's own initialization, before clap ever gets a
    // chance to handle a malformed argument; `args()` panics outright on a
    // non-UTF-8 arg (possible on Windows/odd shells), which would crash the
    // whole CLI for a command that doesn't even use `--env`.
    requested_env_from(
        std::env::args_os()
            .skip(1)
            .map(|arg| arg.to_string_lossy().into_owned()),
    )
}

/// Pure scan over an arg iterator — unit-testable without touching the real
/// process argv. See [`requested_env_from_argv`] for why this exists.
///
/// Scans the *entire* argv and keeps the *last* non-empty `--env`/`--env=`
/// value, rather than stopping at the first match. A global `--env` and a
/// command-local one sharing the same arg id (e.g. `gddy --env bar pat add
/// --env foo ...`) can both appear in one invocation; whichever clap
/// resolves as the effective value (empirically, the last one) is the one
/// this scan must agree with, or a real env-var-only environment can be
/// rejected as unknown even though it's the value actually in effect. An
/// empty value (`--env=` with nothing after the `=`, or `--env` immediately
/// followed by another flag with nothing captured) is ignored rather than
/// becoming a literal empty-string candidate.
fn requested_env_from(mut args: impl Iterator<Item = String>) -> Option<String> {
    let mut result = None;
    while let Some(arg) = args.next() {
        let value = if let Some(v) = arg.strip_prefix("--env=") {
            Some(v.to_owned())
        } else if arg == "--env" {
            args.next()
        } else {
            None
        };
        if let Some(v) = value.filter(|v| !v.is_empty()) {
            result = Some(v);
        }
    }
    result
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

/// Builds the full `Environments` instance for candidate default
/// `default_raw`, falling back to [`DEFAULT_ENV`] if it can't actually
/// resolve. A corrupted/hand-edited `.gdenv`, or one naming a since-removed
/// custom env, must not become the CLI's real startup default — every other
/// command relying on the default active environment (via
/// `ctx.middleware.env`) would then fail with "unknown environment" instead
/// of falling back, unlike `env::get_env`'s own `is_known` guard. Validated
/// against `probe` directly (a plain method call on this local instance),
/// not `instance()`/`resolve()`, so this can't re-enter the singleton's own
/// initialization. Split out from [`build_environments`] for testability
/// without touching the real `.gdenv` file or the process-wide singleton.
fn resolve_default_environments(default_raw: &str) -> Environments {
    let probe = register(Environments::new(default_raw.to_owned()), default_raw);
    if probe.resolve(default_raw).is_ok() {
        probe
    } else {
        register(Environments::new(DEFAULT_ENV), default_raw)
    }
}

/// Registers the compiled `ote`/`prod` defaults plus any env-var-only
/// placeholder (see the module doc) onto `envs`. `default_candidate` is the
/// `.gdenv` value under consideration as the active default — pre-registered
/// like any other candidate so [`build_environments`] can validate it even
/// when it's only defined via `<PREFIX>_API_URL`.
fn register(envs: Environments, default_candidate: &str) -> Environments {
    let mut envs = envs.with_app_id(APP_ID).with_config_file(true);
    for b in BUILTINS {
        envs = envs.with_environment(
            b.name,
            EnvironmentDef::new()
                .with_client_id(b.client_id)
                .with_auth_url(derive_auth_url(b.api_url))
                .with_token_url(derive_token_url(b.api_url))
                .with_field("api_url", b.api_url),
        );
    }
    // Pre-register a placeholder for any candidate (the `.gdenv` default, or
    // an explicit `--env <name>` on the command line) that's otherwise
    // unknown to every layer but has a `<PREFIX>_API_URL` env var set —
    // cli-engine's own env-var layer only overrides a bag key already
    // present in a resolved record, it can't introduce a brand-new
    // selectable environment on its own. An empty `api_url` placeholder
    // gives that layer something to fill in; it's never registered for a
    // name already known via a compiled/file entry, so a real value is
    // never clobbered.
    for candidate in [default_candidate.to_owned()]
        .into_iter()
        .chain(requested_env_from_argv())
    {
        if envs.list().contains(&candidate) {
            continue;
        }
        let prefix = env_prefix(&candidate);
        // `clean_url`, not a bare `.is_ok()` — a set-but-blank
        // `<PREFIX>_API_URL` (e.g. `""`) would otherwise make cli-engine's
        // own `--env <name>` validation accept the name, only for `adapt` to
        // reject it moments later with a confusing "no usable api_url"
        // error instead of a clean "unknown environment" one.
        let has_usable_api_url = std::env::var(format!("{prefix}_API_URL"))
            .ok()
            .and_then(|v| clean_url(&v))
            .is_some();
        if has_usable_api_url {
            envs =
                envs.with_environment(candidate, EnvironmentDef::new().with_field("api_url", ""));
        }
    }
    envs
}

/// The shared `Environments` instance — built once, reused by every consumer
/// (`main.rs`'s `CliConfig::with_environments`, `auth.rs`'s
/// `PkceAuthProvider::with_environments`, and this module's own `resolve`/
/// `listable`/`is_known`).
pub fn instance() -> &'static Arc<Environments> {
    static INSTANCE: OnceLock<Arc<Environments>> = OnceLock::new();
    INSTANCE.get_or_init(|| Arc::new(build_environments()))
}

/// Adapts a resolved `cli_engine::environments::Environment` into gddy's own
/// [`ResolvedEnv`] shape, applying the derivation/defaulting and URL
/// validation that cli-engine's generic `extra` bag has no notion of.
fn adapt(name: &str, env: Environment) -> cli_engine::Result<ResolvedEnv> {
    let api_url = env
        .extra
        .get("api_url")
        .and_then(|v| clean_url(v))
        .ok_or_else(|| {
            CliCoreError::message(format!(
                "environment {name:?} has no usable api_url configured"
            ))
        })?;
    let oauth = env.oauth.unwrap_or_default();
    // `clean_url`, not a bare emptiness check — a blank-but-non-empty or
    // schemeless override (from a local config entry or an `<ENV>_OAUTH_*`
    // env var) must fall back to the derived endpoint too, matching the
    // validation already applied to `api_url`/`domains_api_url`/`account_url`
    // above.
    let auth_url = clean_url(&oauth.auth_url).unwrap_or_else(|| derive_auth_url(&api_url));
    let token_url = clean_url(&oauth.token_url).unwrap_or_else(|| derive_token_url(&api_url));
    let domains_api_url = env
        .extra
        .get("domains_api_url")
        .and_then(|v| clean_url(v))
        .unwrap_or_else(|| api_url.clone());
    let account_url = env
        .extra
        .get("account_url")
        .and_then(|v| clean_url(v))
        .unwrap_or_else(|| derive_account_url(name));
    Ok(ResolvedEnv {
        name: name.to_owned(),
        api_url,
        client_id: oauth.client_id,
        auth_url,
        token_url,
        domains_api_url,
        account_url,
    })
}

/// Falls back to a `<PREFIX>_API_URL`-only definition for a name unknown to
/// every compiled/file layer — see the module doc for why this must survive
/// the migration onto cli-engine's `Environments` (which cannot define a new
/// environment from an env var alone). Pure: the var getter is injected so
/// this is unit-testable without touching process state.
fn resolve_from_env_var_only(
    name: &str,
    var: impl Fn(&str) -> Option<String>,
) -> Option<ResolvedEnv> {
    let prefix = env_prefix(name);
    let api_url = var(&format!("{prefix}_API_URL")).and_then(|v| clean_url(&v))?;
    let domains_api_url = var(&format!("{prefix}_DOMAINS_API_URL"))
        .and_then(|v| clean_url(&v))
        .unwrap_or_else(|| api_url.clone());
    let account_url = var(&format!("{prefix}_ACCOUNT_URL"))
        .and_then(|v| clean_url(&v))
        .unwrap_or_else(|| derive_account_url(name));
    Some(ResolvedEnv {
        name: name.to_owned(),
        auth_url: derive_auth_url(&api_url),
        token_url: derive_token_url(&api_url),
        api_url,
        client_id: String::new(),
        domains_api_url,
        account_url,
    })
}

/// Resolve an environment by name (built-ins → local config → env var, with a
/// pure-env-var-only fallback for a name none of those layers define).
pub fn resolve(name: &str) -> cli_engine::Result<ResolvedEnv> {
    match instance().resolve(name) {
        Ok(env) => adapt(name, env),
        Err(err) => resolve_from_env_var_only(name, |k| std::env::var(k).ok()).ok_or(err),
    }
}

/// Resolve the domain-command view of an environment: its domains base URL.
/// Thin wrapper over [`resolve`].
pub fn resolve_domains(name: &str) -> cli_engine::Result<ResolvedDomains> {
    let env = resolve(name)?;
    Ok(ResolvedDomains {
        base_url: env.domains_api_url,
    })
}

/// The default environment's built-in API base URL.
///
/// Infallible last-resort value (unlike [`resolve`], which can fail on a
/// malformed local config), so callers never end up with an empty base URL.
pub fn default_api_url() -> &'static str {
    BUILTINS
        .iter()
        .find(|b| b.name == DEFAULT_ENV)
        .map(|b| b.api_url)
        .unwrap_or("https://api.godaddy.com")
}

/// Environments to show in `env list`: built-ins + local-config entries.
/// Env-var-only environments are intentionally excluded (matching cli-engine's
/// own `Environments::list`, which can't discover a name defined only by an
/// env var either).
pub fn listable() -> cli_engine::Result<Vec<ResolvedEnv>> {
    let envs = instance();
    Ok(envs
        .list()
        .into_iter()
        .filter_map(|name| envs.resolve(&name).ok().and_then(|e| adapt(&name, e).ok()))
        .collect())
}

/// Whether `name` is a usable environment (built-in, locally configured, or
/// defined via a `<PREFIX>_API_URL` env var).
pub fn is_known(name: &str) -> bool {
    resolve(name).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_environment(def: EnvironmentDef) -> Environment {
        Environments::new("x")
            .with_environment("x", def)
            .resolve("x")
            .expect("x resolves")
    }

    #[test]
    fn adapt_derives_oauth_urls_from_api_url_when_unset() {
        let env = test_environment(
            EnvironmentDef::new()
                .with_client_id("cid")
                .with_field("api_url", "https://api.example.test"),
        );
        let resolved = adapt("dev", env).expect("adapts");
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
    fn adapt_prefers_explicit_oauth_urls_over_derived() {
        let env = test_environment(
            EnvironmentDef::new()
                .with_client_id("cid")
                .with_auth_url("https://auth.example.test/authorize")
                .with_token_url("https://auth.example.test/token")
                .with_field("api_url", "https://api.example.test"),
        );
        let resolved = adapt("dev", env).expect("adapts");
        assert_eq!(resolved.auth_url, "https://auth.example.test/authorize");
        assert_eq!(resolved.token_url, "https://auth.example.test/token");
    }

    #[test]
    fn adapt_falls_back_to_derived_oauth_urls_when_override_is_blank_or_malformed() {
        // A blank-but-non-empty or schemeless override (e.g. from
        // `<ENV>_OAUTH_AUTH_URL=" "`) must not be used as-is — it should be
        // treated the same as unset, matching the validation already applied
        // to api_url/domains_api_url/account_url.
        let env = test_environment(
            EnvironmentDef::new()
                .with_client_id("cid")
                .with_auth_url("   ")
                .with_token_url("not-a-url")
                .with_field("api_url", "https://api.example.test"),
        );
        let resolved = adapt("dev", env).expect("adapts");
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
    fn adapt_rejects_missing_api_url() {
        let env = test_environment(EnvironmentDef::new().with_client_id("cid"));
        let err = adapt("dev", env).expect_err("no api_url");
        assert!(err.to_string().contains("dev"));
    }

    #[test]
    fn adapt_rejects_blank_api_url_final_value() {
        // A blank override that made it all the way through cli-engine's own
        // (unconditional) layering must still be treated as unset here.
        let env = test_environment(
            EnvironmentDef::new()
                .with_client_id("cid")
                .with_field("api_url", "   "),
        );
        assert!(adapt("dev", env).is_err());
    }

    #[test]
    fn domains_api_url_defaults_to_api_url() {
        let env = test_environment(
            EnvironmentDef::new()
                .with_client_id("cid")
                .with_field("api_url", "https://api.example.test"),
        );
        let resolved = adapt("dev", env).expect("adapts");
        assert_eq!(resolved.domains_api_url, resolved.api_url);
    }

    #[test]
    fn domains_api_url_override_is_respected() {
        let env = test_environment(
            EnvironmentDef::new()
                .with_client_id("cid")
                .with_field("api_url", "https://api.example.test")
                .with_field("domains_api_url", "https://domains.example.test"),
        );
        let resolved = adapt("dev", env).expect("adapts");
        assert_eq!(resolved.domains_api_url, "https://domains.example.test");
    }

    #[test]
    fn account_url_defaults_to_bare_domain_for_prod() {
        let env = test_environment(
            EnvironmentDef::new()
                .with_client_id("cid")
                .with_field("api_url", "https://api.godaddy.com"),
        );
        let resolved = adapt("prod", env).expect("adapts");
        assert_eq!(resolved.account_url, "https://account.godaddy.com");
    }

    #[test]
    fn account_url_defaults_to_prefixed_domain_for_non_prod() {
        let env = test_environment(
            EnvironmentDef::new()
                .with_client_id("cid")
                .with_field("api_url", "https://api.ote-godaddy.com"),
        );
        let resolved = adapt("ote", env).expect("adapts");
        assert_eq!(resolved.account_url, "https://account.ote-godaddy.com");
    }

    #[test]
    fn account_url_override_is_respected() {
        let env = test_environment(
            EnvironmentDef::new()
                .with_client_id("cid")
                .with_field("api_url", "https://api.example.test")
                .with_field("account_url", "https://account.override.test"),
        );
        let resolved = adapt("dev", env).expect("adapts");
        assert_eq!(resolved.account_url, "https://account.override.test");
    }

    #[test]
    fn env_var_only_fallback_defines_a_new_env() {
        let var = |k: &str| (k == "DEV_API_URL").then(|| "https://dev.example.test".to_owned());
        let resolved = resolve_from_env_var_only("dev", var).expect("dev resolves from env var");
        assert_eq!(resolved.api_url, "https://dev.example.test");
        assert_eq!(
            resolved.auth_url,
            "https://dev.example.test/v2/oauth2/authorize"
        );
        assert!(resolved.client_id.is_empty());
    }

    #[test]
    fn env_var_only_fallback_is_none_without_the_var() {
        let var = |_: &str| None;
        assert!(resolve_from_env_var_only("dev", var).is_none());
    }

    #[test]
    fn env_var_only_fallback_respects_domains_and_account_overrides() {
        let var = |k: &str| match k {
            "DEV_API_URL" => Some("https://dev.example.test".to_owned()),
            "DEV_DOMAINS_API_URL" => Some("https://domains.dev.example.test".to_owned()),
            "DEV_ACCOUNT_URL" => Some("https://account.dev.example.test".to_owned()),
            _ => None,
        };
        let resolved = resolve_from_env_var_only("dev", var).expect("dev resolves");
        assert_eq!(resolved.domains_api_url, "https://domains.dev.example.test");
        assert_eq!(resolved.account_url, "https://account.dev.example.test");
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
    fn default_api_url_is_the_builtin_prod_url() {
        assert_eq!(default_api_url(), "https://api.godaddy.com");
    }

    #[test]
    fn env_prefix_uppercases_and_replaces_hyphen() {
        assert_eq!(env_prefix("ote"), "OTE");
        assert_eq!(env_prefix("prod-us"), "PROD_US");
    }

    #[test]
    fn resolve_default_environments_falls_back_to_default_env_for_an_unresolvable_gdenv_value() {
        // A corrupted/stale `.gdenv` value that resolves to nothing (no
        // compiled/file entry, no matching `<PREFIX>_API_URL`) must not
        // become the CLI's real startup default.
        let envs = resolve_default_environments("totally-bogus-env-name");
        assert_eq!(envs.default_env(), DEFAULT_ENV);
        assert!(envs.resolve(DEFAULT_ENV).is_ok());
    }

    #[test]
    fn resolve_default_environments_keeps_a_resolvable_gdenv_value() {
        let envs = resolve_default_environments("prod");
        assert_eq!(envs.default_env(), "prod");
    }

    fn argv(args: &[&str]) -> impl Iterator<Item = String> {
        args.iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
            .into_iter()
    }

    #[test]
    fn requested_env_from_finds_space_separated_value() {
        assert_eq!(
            requested_env_from(argv(&["--dry-run", "--env", "dev", "list"])),
            Some("dev".to_owned())
        );
    }

    #[test]
    fn requested_env_from_finds_equals_separated_value() {
        assert_eq!(
            requested_env_from(argv(&["--env=dev", "list"])),
            Some("dev".to_owned())
        );
    }

    #[test]
    fn requested_env_from_is_none_without_the_flag() {
        assert_eq!(requested_env_from(argv(&["env", "list"])), None);
    }

    #[test]
    fn requested_env_from_trailing_env_flag_with_no_value_is_none() {
        assert_eq!(requested_env_from(argv(&["--env"])), None);
    }

    #[test]
    fn requested_env_from_keeps_the_last_of_multiple_occurrences() {
        // A global `--env` and a command-local one sharing the same arg id
        // can both appear (e.g. `gddy --env bar pat add --env foo ...`);
        // clap resolves the *last* one as effective, so this scan must too.
        assert_eq!(
            requested_env_from(argv(&[
                "--env",
                "bar",
                "pat",
                "add",
                "--env",
                "foo",
                "test-token"
            ])),
            Some("foo".to_owned())
        );
    }

    #[test]
    fn requested_env_from_ignores_an_empty_equals_value() {
        assert_eq!(requested_env_from(argv(&["--env="])), None);
    }

    #[test]
    fn requested_env_from_empty_occurrence_does_not_clobber_an_earlier_real_value() {
        assert_eq!(
            requested_env_from(argv(&["--env", "dev", "--env="])),
            Some("dev".to_owned())
        );
    }
}
