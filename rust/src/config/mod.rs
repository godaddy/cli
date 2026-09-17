use serde::{Deserialize, Serialize};

mod native_extension;
mod settings;
pub(crate) mod settings_form;

pub use settings::{SettingConfig, SettingIcon};

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
    #[serde(default)]
    pub settings: Vec<SettingConfig>,
    #[serde(default)]
    pub native_extension: Option<NativeExtensionConfig>,
}

impl Config {
    /// Validate required field shapes for a `godaddy.toml` manifest.
    pub fn validate(&self) -> Result<(), ConfigError> {
        let mut errors = Vec::new();

        if !is_valid_app_name(&self.name) {
            errors.push(format!(
                "name must match /^[a-z0-9-]{{3,255}}$/ (got {:?})",
                self.name
            ));
        }
        if !is_uuid_v4(&self.client_id) {
            errors.push(format!(
                "client_id must be a UUID v4 (got {:?})",
                self.client_id
            ));
        }
        if !is_semver(&self.version) {
            errors.push(format!(
                "version must be a semver string (got {:?})",
                self.version
            ));
        }
        if !is_absolute_http_url(&self.url) {
            errors.push(format!(
                "url must be an absolute http(s) URL (got {:?})",
                self.url
            ));
        }
        if !is_absolute_http_url(&self.proxy_url) {
            errors.push(format!(
                "proxy_url must be an absolute http(s) URL (got {:?})",
                self.proxy_url
            ));
        }
        if self.authorization_scopes.is_empty() {
            errors.push("authorization_scopes must contain at least one scope".to_owned());
        }

        for (i, action) in self.actions.iter().enumerate() {
            validate_action(
                &mut errors,
                &format!("actions[{i}]"),
                action,
                &self.proxy_url,
            );
        }

        if let Some(subscriptions) = &self.subscriptions {
            for (i, sub) in subscriptions.webhook.iter().enumerate() {
                validate_subscription(
                    &mut errors,
                    &format!("subscriptions.webhook[{i}]"),
                    sub,
                    &self.proxy_url,
                );
            }
        }

        for (i, deps) in self.dependencies.iter().enumerate() {
            for (j, dep) in deps.app.iter().enumerate() {
                validate_dependency(&mut errors, &format!("dependencies[{i}].app[{j}]"), dep);
            }
            for (j, dep) in deps.feature.iter().enumerate() {
                validate_dependency(&mut errors, &format!("dependencies[{i}].feature[{j}]"), dep);
            }
        }

        if let Some(extensions) = &self.extensions {
            for (i, ext) in extensions.embed.iter().enumerate() {
                validate_named_extension(
                    &mut errors,
                    &format!("extensions.embed[{i}]"),
                    &ext.name,
                    &ext.handle,
                    &ext.source,
                    &ext.targets,
                );
            }
            for (i, ext) in extensions.checkout.iter().enumerate() {
                validate_named_extension(
                    &mut errors,
                    &format!("extensions.checkout[{i}]"),
                    &ext.name,
                    &ext.handle,
                    &ext.source,
                    &ext.targets,
                );
            }
            if let Some(blocks) = &extensions.blocks {
                require_non_empty(&mut errors, "extensions.blocks.source", &blocks.source);
            }
        }

        if let Some(native) = &self.native_extension {
            native_extension::validate(
                &mut errors,
                "native_extension",
                &native.support_contact,
                &native.android_package_name,
            );
        }

        settings::validate_settings(&self.settings, &mut errors);

        if errors.is_empty() {
            Ok(())
        } else {
            Err(ConfigError::Validation(errors.join("; ")))
        }
    }
}

const MIN_IDENT_LEN: usize = 3;

fn require_min_len(errors: &mut Vec<String>, path: &str, value: &str, min: usize) {
    if value.chars().count() < min {
        errors.push(format!(
            "{path} must be at least {min} characters (got {value:?})"
        ));
    }
}

fn require_non_empty(errors: &mut Vec<String>, path: &str, value: &str) {
    if value.is_empty() {
        errors.push(format!("{path} must be non-empty"));
    }
}

/// True when `name` matches `/^[a-z0-9-]{3,255}$/`.
pub(crate) fn is_valid_app_name(name: &str) -> bool {
    let len = name.len();
    (3..=255).contains(&len)
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn is_uuid_v4(value: &str) -> bool {
    let Ok(id) = uuid::Uuid::parse_str(value) else {
        return false;
    };
    id.get_version() == Some(uuid::Version::Random)
        && matches!(id.get_variant(), uuid::Variant::RFC4122)
}

fn is_semver(value: &str) -> bool {
    semver::Version::parse(value).is_ok()
}

fn is_absolute_http_url(value: &str) -> bool {
    url::Url::parse(value).is_ok_and(|url| matches!(url.scheme(), "http" | "https"))
}

/// Resolve `endpoint` against `proxy_url` (absolute or proxy-relative).
fn is_endpoint_url(endpoint: &str, proxy_url: &str) -> bool {
    let Ok(base) = url::Url::parse(proxy_url) else {
        return false;
    };
    url::Url::options()
        .base_url(Some(&base))
        .parse(endpoint)
        .is_ok_and(|url| matches!(url.scheme(), "http" | "https"))
}

/// Reduce `full_url` to a path relative to `proxy_url` when they share a
/// scheme, host, and port; otherwise return `full_url` unchanged
pub fn relativize_webhook_url(full_url: &str, proxy_url: &str) -> String {
    let (Ok(full), Ok(base)) = (url::Url::parse(full_url), url::Url::parse(proxy_url)) else {
        return full_url.to_owned();
    };
    if full.scheme() != base.scheme()
        || full.host_str() != base.host_str()
        || full.port_or_known_default() != base.port_or_known_default()
    {
        return full_url.to_owned();
    }
    let mut relative = full.path().to_owned();
    if let Some(query) = full.query() {
        relative.push('?');
        relative.push_str(query);
    }
    if let Some(fragment) = full.fragment() {
        relative.push('#');
        relative.push_str(fragment);
    }
    relative
}

fn validate_action(errors: &mut Vec<String>, path: &str, action: &ActionConfig, proxy_url: &str) {
    require_min_len(errors, &format!("{path}.name"), &action.name, MIN_IDENT_LEN);
    if !is_endpoint_url(&action.url, proxy_url) {
        errors.push(format!(
            "{path}.url must be a valid endpoint relative to proxy_url (got {:?})",
            action.url
        ));
    }
}

fn validate_subscription(
    errors: &mut Vec<String>,
    path: &str,
    sub: &SubscriptionConfig,
    proxy_url: &str,
) {
    require_min_len(errors, &format!("{path}.name"), &sub.name, MIN_IDENT_LEN);
    if sub.events.is_empty() {
        errors.push(format!("{path}.events must contain at least one event"));
    }
    if !is_endpoint_url(&sub.url, proxy_url) {
        errors.push(format!(
            "{path}.url must be a valid endpoint relative to proxy_url (got {:?})",
            sub.url
        ));
    }
}

fn validate_dependency(errors: &mut Vec<String>, path: &str, dep: &DependencyConfig) {
    require_min_len(errors, &format!("{path}.name"), &dep.name, MIN_IDENT_LEN);
    if let Some(version) = &dep.version
        && !is_semver(version)
    {
        errors.push(format!(
            "{path}.version must be a semver string (got {version:?})"
        ));
    }
}

fn validate_named_extension(
    errors: &mut Vec<String>,
    path: &str,
    name: &str,
    handle: &str,
    source: &str,
    targets: &[ExtensionTarget],
) {
    require_min_len(errors, &format!("{path}.name"), name, MIN_IDENT_LEN);
    require_min_len(errors, &format!("{path}.handle"), handle, MIN_IDENT_LEN);
    require_non_empty(errors, &format!("{path}.source"), source);
    if targets.is_empty() {
        errors.push(format!("{path}.targets must contain at least one target"));
    }
    for (i, target) in targets.iter().enumerate() {
        require_non_empty(
            errors,
            &format!("{path}.targets[{i}].target"),
            &target.target,
        );
    }
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativeExtensionConfig {
    #[serde(default)]
    pub name: Option<String>,
    pub support_contact: String,
    pub android_package_name: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("config file not found at {path}")]
    NotFound { path: String },
    #[error("failed to read config: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to parse config: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("invalid config: {0}")]
    Validation(String),
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
    config.validate()?;
    Ok(config)
}

pub fn write_config(path: &std::path::Path, config: &Config) -> Result<(), ConfigError> {
    config.validate()?;
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
mod tests;
