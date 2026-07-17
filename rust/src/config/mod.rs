use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub name: String,
    pub client_id: String,
    #[serde(default)]
    pub description: Option<String>,
    pub version: String,
    pub url: String,
    pub proxy_url: String,
    pub authorization_scopes: Vec<String>,
    #[serde(default)]
    pub actions: Vec<ActionConfig>,
    #[serde(default)]
    pub subscriptions: Option<SubscriptionsConfig>,
    #[serde(default)]
    pub dependencies: Vec<DependenciesConfig>,
    #[serde(default)]
    pub extensions: Option<ExtensionsConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionConfig {
    pub name: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionsConfig {
    #[serde(default)]
    pub webhook: Vec<SubscriptionConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionConfig {
    pub name: String,
    pub events: Vec<String>,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependenciesConfig {
    #[serde(default)]
    pub app: Vec<DependencyConfig>,
    #[serde(default)]
    pub feature: Vec<DependencyConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyConfig {
    pub name: String,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionsConfig {
    #[serde(default)]
    pub embed: Vec<EmbedExtensionConfig>,
    #[serde(default)]
    pub checkout: Vec<CheckoutExtensionConfig>,
    pub blocks: Option<BlocksExtensionConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbedExtensionConfig {
    pub name: String,
    pub handle: String,
    pub source: String,
    #[serde(default)]
    pub targets: Vec<ExtensionTarget>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckoutExtensionConfig {
    pub name: String,
    pub handle: String,
    pub source: String,
    #[serde(default)]
    pub targets: Vec<ExtensionTarget>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlocksExtensionConfig {
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionTarget {
    pub target: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("config file not found at {path}")]
    NotFound { path: String },
    #[error("failed to read config: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to parse config: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("failed to serialize config: {0}")]
    Serialize(#[from] toml::ser::Error),
}

pub fn read_config(path: &std::path::Path) -> Result<Config, ConfigError> {
    if !path.exists() {
        return Err(ConfigError::NotFound {
            path: path.display().to_string(),
        });
    }
    let contents = std::fs::read_to_string(path)?;
    let config: Config = toml::from_str(&contents)?;
    Ok(config)
}

pub fn write_config(path: &std::path::Path, config: &Config) -> Result<(), ConfigError> {
    let contents = toml::to_string_pretty(config)?;
    std::fs::write(path, contents)?;
    Ok(())
}

/// Returns the config file path for a given env.
///
/// - `None` / `"prod"` → `godaddy.toml`
/// - other envs → `godaddy.<env>.toml`
pub fn config_path(env: Option<&str>) -> std::path::PathBuf {
    match env {
        None | Some("prod") => std::path::PathBuf::from("godaddy.toml"),
        Some(e) => std::path::PathBuf::from(format!("godaddy.{e}.toml")),
    }
}

/// Path to the env file for a given env, parallel to [`config_path`]:
/// `None` / `"prod"` → `.env`, other envs → `.env.<env>`.
pub fn env_path(env: Option<&str>) -> std::path::PathBuf {
    match env {
        None | Some("prod") => std::path::PathBuf::from(".env"),
        Some(e) => std::path::PathBuf::from(format!(".env.{e}")),
    }
}

/// JSON-encode after stripping NULs, so a newline
///  / `#` / `=` in a secret can't corrupt the `.env`.
fn format_env_value(value: &str) -> String {
    serde_json::to_string(&value.replace('\0', "")).unwrap_or_default()
}

/// Build the new `.env`: overwrite the four `GODADDY_*` keys in place, keep every
/// other line verbatim, and append any key the file lacked.
fn merge_env_content(
    existing: Option<&str>,
    secret: &str,
    public_key: &str,
    client_id: &str,
    client_secret: &str,
) -> String {
    let owned = [
        ("GODADDY_WEBHOOK_SECRET", secret),
        ("GODADDY_PUBLIC_KEY", public_key),
        ("GODADDY_CLIENT_ID", client_id),
        ("GODADDY_CLIENT_SECRET", client_secret),
    ];
    let render = |name: &str, value: &str| format!("{name}={}", format_env_value(value));

    let mut seen = [false; 4];
    let mut out: Vec<String> = Vec::new();

    // Rewrite our keys where they sit; every other line passes through unchanged.
    for line in existing.unwrap_or_default().lines() {
        let idx = if line.trim_start().starts_with('#') {
            None
        } else {
            line.split_once('=')
                .and_then(|(k, _)| owned.iter().position(|&(name, _)| name == k.trim()))
        };
        match idx {
            Some(i) if !seen[i] => {
                out.push(render(owned[i].0, owned[i].1));
                seen[i] = true;
            }
            Some(_) => {}
            None => out.push(line.to_string()),
        }
    }

    // Append any key the file didn't already contain.
    for (i, &(name, value)) in owned.iter().enumerate() {
        if !seen[i] {
            out.push(render(name, value));
        }
    }

    out.join("\n")
}

/// Write the app secrets into the env file for `env`, preserving existing content.
pub fn write_env_file(
    env: Option<&str>,
    secret: &str,
    public_key: &str,
    client_id: &str,
    client_secret: &str,
) -> Result<(), ConfigError> {
    let path = env_path(env);
    let existing = std::fs::read_to_string(&path).ok();
    let contents = merge_env_content(
        existing.as_deref(),
        secret,
        public_key,
        client_id,
        client_secret,
    );
    std::fs::write(&path, contents)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_path_matches_convention() {
        use std::path::Path;
        assert_eq!(env_path(None), Path::new(".env"));
        assert_eq!(env_path(Some("prod")), Path::new(".env"));
        assert_eq!(env_path(Some("ote")), Path::new(".env.ote"));
    }

    #[test]
    fn merge_env_content_writes_fresh_keys() {
        let result = merge_env_content(None, "secret", "pubkey", "cid", "csecret");
        assert!(result.contains(r#"GODADDY_WEBHOOK_SECRET="secret""#));
        assert!(result.contains(r#"GODADDY_PUBLIC_KEY="pubkey""#));
        assert!(result.contains(r#"GODADDY_CLIENT_ID="cid""#));
        assert!(result.contains(r#"GODADDY_CLIENT_SECRET="csecret""#));
    }

    #[test]
    fn merge_env_content_preserves_existing() {
        let existing = "FOO=bar\n# note\nGODADDY_CLIENT_ID=\"old\"";
        let result = merge_env_content(Some(existing), "secret", "pubkey", "new_cid", "csecret");
        assert!(result.contains("FOO=bar"));
        assert!(result.contains("# note"));
        assert!(result.contains(r#"GODADDY_CLIENT_ID="new_cid""#));
        assert!(!result.contains("\"old\""));
    }

    #[test]
    fn merge_env_content_dedupes_owned_keys() {
        let existing = "GODADDY_CLIENT_ID=a\nGODADDY_CLIENT_ID=b";
        let result = merge_env_content(Some(existing), "s", "p", "new", "cs");
        assert_eq!(result.matches("GODADDY_CLIENT_ID=").count(), 1);
        assert!(result.contains(r#"GODADDY_CLIENT_ID="new""#));
    }
}
