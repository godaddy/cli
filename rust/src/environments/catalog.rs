//! Environment-specific base-URL resolution for API-catalog domains (used by
//! `api domain list` / `api call`).

use cli_engine::ConfigSource;

use super::config::{clean_url, substitute_env_host};
use super::{DEFAULT_ENV, env_prefix, instance};

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
