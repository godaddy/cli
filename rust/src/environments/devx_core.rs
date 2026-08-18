//! DevX Core API gateway base-URL resolution per environment.

use super::config::clean_url;
use super::env_prefix;

/// DevX Core API gateway base URL for each compiled-in builtin, consulted by
/// [`devx_core_url_with`] only after both env-var override tiers miss.
const BUILTIN_DEVX_CORE_URLS: &[(&str, &str)] = &[
    ("ote", "https://api.developer.commerce.ote-godaddy.com"),
    ("prod", "https://api.developer.commerce.godaddy.com"),
];

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

#[cfg(test)]
mod tests {
    use super::*;

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
