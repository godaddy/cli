use super::*;
use cli_engine::environments::EnvTable;
use std::sync::Mutex;

// Serializes every test that touches real process env vars, so
// parallel test threads can't observe each other's GDDY_* overrides.
static ENV_LOCK: Mutex<()> = Mutex::new(());

/// RAII guard that sets an env var and restores it to its prior state on
/// drop — removing it if it wasn't already set, or putting the original
/// value back if it was — even if a test panics. Restoring rather than
/// unconditionally removing keeps a var a developer happens to already
/// have set in their shell from leaking into the rest of the test run.
struct EnvGuard {
    key: &'static str,
    prior: Option<String>,
}
impl EnvGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let prior = std::env::var(key).ok();
        // SAFETY: caller holds ENV_LOCK.
        #[allow(unsafe_code)]
        unsafe {
            std::env::set_var(key, value);
        }
        Self { key, prior }
    }
}
impl Drop for EnvGuard {
    fn drop(&mut self) {
        // SAFETY: caller holds ENV_LOCK; restore on any exit incl. panic.
        #[allow(unsafe_code)]
        unsafe {
            match &self.prior {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }
}

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

#[test]
fn email_api_url_resolves_known_environments() {
    for (name, url) in [
        ("prod", "https://productivity.api.godaddy.com"),
        ("test", "https://productivity.api.test-godaddy.com"),
        ("stage", "https://productivity.api.stg-godaddy.com"),
    ] {
        let resolved = test_environment(name, |t| {
            t.with("client_id", "cid")
                .with("api_url", "https://api.example.test")
        });
        assert_eq!(resolved.email_api_url, url, "environment {name:?}");
    }
}

#[test]
fn email_api_url_aliases_ote_to_test() {
    let resolved = test_environment("ote", |t| {
        t.with("client_id", "cid")
            .with("api_url", "https://api.ote-godaddy.com")
    });
    assert_eq!(
        resolved.email_api_url,
        "https://productivity.api.test-godaddy.com"
    );
}

#[test]
fn email_api_url_falls_back_to_api_url_for_an_unknown_environment() {
    let resolved = test_environment("dev", |t| {
        t.with("client_id", "cid")
            .with("api_url", "https://api.example.test")
    });
    assert_eq!(resolved.email_api_url, "https://api.example.test");
}

#[test]
fn email_api_url_override_is_respected() {
    let resolved = test_environment("prod", |t| {
        t.with("client_id", "cid")
            .with("api_url", "https://api.example.test")
            .with("email_api_url", "https://email.override.test")
    });
    assert_eq!(resolved.email_api_url, "https://email.override.test");
}

#[test]
fn env_var_overrides_email_api_url() {
    let _g = ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _guard = EnvGuard::set("GDDY_EMAIL_API_URL", "https://email.override.test");

    let resolved = test_environment_with_app_id("dev", |t| {
        t.with("client_id", "cid")
            .with("api_url", "https://api.example.test")
    });
    assert_eq!(resolved.email_api_url, "https://email.override.test");
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
    let _guard = EnvGuard::set("GDDY_AUTH_URL", "https://auth.override.test");

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
    let _guard = EnvGuard::set("GDDY_TOKEN_URL", "https://token.override.test");

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
    let _guard = EnvGuard::set("GDDY_DOMAINS_API_URL", "https://domains.override.test");

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
    let _guard = EnvGuard::set("GDDY_ACCOUNT_URL", "https://account.override.test");

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
    let _guard = EnvGuard::set("GDDY_AUTH_URL", "not-a-url");

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
    let _guard = EnvGuard::set("GDDY_AUTH_URL", "   ");

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
