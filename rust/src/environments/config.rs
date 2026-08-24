//! `GddyEnvConfig`: the fully-resolved environment config assembled by
//! cli-engine's `EnvConfig` derive from compiled-in defaults, an
//! `environments.toml` file, and `GDDY_*` env var overrides — plus the URL
//! validation and GoDaddy hostname-convention helpers its fields lean on.

use cli_engine::{EnvConfig, SourceChain};

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

    /// Base URL for the DevX Core API gateway used by onboarding. Custom
    /// environments set this in `environments.toml`; built-in environments
    /// receive their defaults from `BaseEnvConfig`. Shell overrides are
    /// applied separately by `devx_core_url` so their legacy precedence is
    /// preserved.
    #[env_config(
        from_toml = parse_url_from_toml,
        default_fn = default_devx_core_url
    )]
    pub devx_core_url: String,

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

fn default_devx_core_url(_sources: &SourceChain<'_>) -> String {
    // A custom environment must configure this value explicitly. The empty
    // default keeps the field optional for unrelated CLI commands; callers
    // that require DevX Core report a missing URL through `devx_core_url`.
    String::new()
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
///
/// `pub(super)`: also used by [`super::catalog::resolve_catalog_base_url`].
pub(super) fn substitute_env_host(url: &str, env_name: &str) -> Option<String> {
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
///
/// `pub(super)`: also used by [`super::catalog::domain_override`] and
/// [`super::devx_core::devx_core_url_with`].
pub(super) fn clean_url(raw: &str) -> Option<String> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::environments::APP_ID;
    use crate::environments::test_support::{ENV_LOCK, EnvGuard};
    use cli_engine::environments::{EnvTable, Environments};

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
}
