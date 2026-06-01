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
