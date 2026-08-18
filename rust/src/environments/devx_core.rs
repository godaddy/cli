//! DevX Core API gateway base-URL resolution per environment.

use super::config::clean_url;
use super::{env_prefix, resolve};

/// Base URL for the DevX Core API gateway for the given environment.
///
/// The configured `devx_core_url` is the default for custom environments.
/// `<PREFIX>_DEVX_CORE_URL` (for example, `DEV_DEVX_CORE_URL`) and the global
/// `DEVX_CORE_URL` shell variable retain precedence over that value. `prod`
/// and `ote` receive their defaults from the compiled-in environment config.
pub fn devx_core_url(name: &str) -> Option<String> {
    let configured = resolve(name)
        .ok()
        .and_then(|config| clean_url(&config.devx_core_url));
    devx_core_url_with(name, configured.as_deref(), |key| std::env::var(key).ok())
}

fn devx_core_url_with(
    name: &str,
    configured: Option<&str>,
    var: impl Fn(&str) -> Option<String>,
) -> Option<String> {
    let prefix = env_prefix(name);
    var(&format!("{prefix}_DEVX_CORE_URL"))
        .and_then(|value| clean_url(&value))
        .or_else(|| var("DEVX_CORE_URL").and_then(|value| clean_url(&value)))
        .or_else(|| configured.and_then(clean_url))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn devx_core_url_uses_prod_and_ote_builtins() {
        assert_eq!(
            devx_core_url_with(
                "prod",
                Some("https://api.developer.commerce.godaddy.com"),
                |_| None,
            )
            .as_deref(),
            Some("https://api.developer.commerce.godaddy.com")
        );
        assert_eq!(
            devx_core_url_with(
                "ote",
                Some("https://api.developer.commerce.ote-godaddy.com"),
                |_| None,
            )
            .as_deref(),
            Some("https://api.developer.commerce.ote-godaddy.com")
        );
    }

    #[test]
    fn devx_core_url_uses_the_environments_toml_value_for_a_custom_env() {
        assert_eq!(
            devx_core_url_with("dev", Some(" https://dev-core.example.test/ "), |_| None)
                .as_deref(),
            Some("https://dev-core.example.test")
        );
    }

    #[test]
    fn devx_core_url_global_override_wins_over_the_environments_toml_value() {
        assert_eq!(
            devx_core_url_with(
                "prod",
                Some("https://configured-core.example.test"),
                |key| { (key == "DEVX_CORE_URL").then(|| " http://localhost:4000/ ".to_owned()) }
            )
            .as_deref(),
            Some("http://localhost:4000")
        );
    }

    #[test]
    fn devx_core_url_per_environment_override_wins_over_global() {
        assert_eq!(
            devx_core_url_with(
                "dev",
                Some("https://configured-core.example.test"),
                |key| match key {
                    "DEV_DEVX_CORE_URL" => Some("https://dev-core.example.test/".to_owned()),
                    "DEVX_CORE_URL" => Some("https://shared-core.example.test".to_owned()),
                    _ => None,
                },
            )
            .as_deref(),
            Some("https://dev-core.example.test")
        );
    }

    #[test]
    fn devx_core_url_custom_env_requires_override() {
        assert_eq!(devx_core_url_with("dev", None, |_| None), None);
    }
}
