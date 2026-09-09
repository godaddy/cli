//! Order Management Shopping API base-URL resolution per environment.

use super::config::clean_url;
use super::{env_prefix, resolve};

/// Base URL for the direct Order Management Shopping API for `name`.
///
/// Until the service is available through the public front door, configure its
/// explicit Katana endpoint in `environments.toml` as `shopping_url`. Shell
/// overrides take precedence: `<PREFIX>_SHOPPING_URL` (for example,
/// `TEST_SHOPPING_URL`), then `SHOPPING_URL`.
pub fn shopping_url(name: &str) -> Option<String> {
    let configured = resolve(name)
        .ok()
        .and_then(|config| clean_url(&config.shopping_url));
    shopping_url_with(name, configured.as_deref(), |key| std::env::var(key).ok())
}

fn shopping_url_with(
    name: &str,
    configured: Option<&str>,
    var: impl Fn(&str) -> Option<String>,
) -> Option<String> {
    let prefix = env_prefix(name);
    var(&format!("{prefix}_SHOPPING_URL"))
        .and_then(|value| clean_url(&value))
        .or_else(|| var("SHOPPING_URL").and_then(|value| clean_url(&value)))
        .or_else(|| configured.and_then(clean_url))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shopping_url_uses_the_environments_toml_value() {
        assert_eq!(
            shopping_url_with("test", Some(" https://shopping.example.test/ "), |_| None)
                .as_deref(),
            Some("https://shopping.example.test")
        );
    }

    #[test]
    fn shopping_url_global_override_wins_over_the_environments_toml_value() {
        assert_eq!(
            shopping_url_with("test", Some("https://configured.example.test"), |key| {
                (key == "SHOPPING_URL").then(|| "http://localhost:8080/".to_owned())
            })
            .as_deref(),
            Some("http://localhost:8080")
        );
    }

    #[test]
    fn shopping_url_per_environment_override_wins_over_global() {
        assert_eq!(
            shopping_url_with(
                "test",
                Some("https://configured.example.test"),
                |key| match key {
                    "TEST_SHOPPING_URL" => Some("https://test-shopping.example.test/".to_owned()),
                    "SHOPPING_URL" => Some("https://shared-shopping.example.test".to_owned()),
                    _ => None,
                }
            )
            .as_deref(),
            Some("https://test-shopping.example.test")
        );
    }

    #[test]
    fn shopping_url_requires_explicit_configuration() {
        assert_eq!(shopping_url_with("prod", None, |_| None), None);
    }
}
