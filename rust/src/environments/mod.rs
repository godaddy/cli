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

mod catalog;
mod config;
mod devx_core;
#[cfg(test)]
mod test_support;

use std::sync::{Arc, LazyLock, OnceLock};

use cli_engine::EnvConfig;
use cli_engine::environments::Environments;

pub use catalog::resolve_catalog_base_url;
pub use config::GddyEnvConfig;
pub use devx_core::devx_core_url;

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

pub fn env_prefix(name: &str) -> String {
    name.to_uppercase().replace('-', "_")
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
mod tests {
    use super::*;
    use crate::environments::test_support::ENV_LOCK;
    use cli_engine::environments::EnvTable;

    #[test]
    fn register_scaffolds_a_file_only_environment() {
        // Resolves through `register()`, which sets `app_id` — so it checks
        // `GDDY_*` overrides and must be serialized against tests that set
        // them (see `ENV_LOCK`'s own doc).
        let _g = ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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
        let _g = ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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
        let _g = ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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
        let _g = ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let envs = resolve_default_environments("totally-bogus-env-name");
        assert_eq!(envs.default_env(), DEFAULT_ENV);
        assert!(envs.source(DEFAULT_ENV).is_ok());
    }

    #[test]
    fn source_existing_but_resolve_failing_is_the_case_resolve_default_environments_must_catch() {
        // `resolve_default_environments`'s own validity check must use
        // `.resolve::<GddyEnvConfig>()`, not `.source()` — a name can be
        // *known* to a layer (so `.source()` succeeds) while still missing
        // required fields (so `.resolve()` fails). Checking only `.source()`
        // would let a misconfigured persisted default stick, instead of
        // falling back to `DEFAULT_ENV`.
        let _g = ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // A file-path override pointing at a nonexistent path, not a bare
        // `register(...)` — without it this would pick up a real developer's
        // own `~/.config/gddy/environments.toml` `[dev]` entry (if any),
        // making the test's result depend on that machine's local config.
        let dir = tempfile::tempdir().expect("tempdir");
        let missing_file = dir.path().join("environments.toml");
        let probe = register(Environments::new("dev").with_config_file_path_override(missing_file))
            .with_environment(
                "dev",
                EnvTable::new().with("api_url", "https://api.example.test"), // no client_id
            );
        assert!(probe.source("dev").is_ok(), "the name is known");
        assert!(
            probe.resolve::<GddyEnvConfig>("dev").is_err(),
            "but it's missing client_id, so it can't actually assemble"
        );
    }

    #[test]
    fn resolve_default_environments_keeps_a_resolvable_gdenv_value() {
        let _g = ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let envs = resolve_default_environments("prod");
        assert_eq!(envs.default_env(), "prod");
    }
}
