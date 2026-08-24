use serde::{Deserialize, Serialize};

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
mod tests {
    use super::settings_form::{
        SettingsFormV1Field, SettingsFormV1Presentation, SettingsFormV1Section,
    };
    use super::*;

    fn valid_config() -> Config {
        Config {
            name: "my-app".to_owned(),
            client_id: "550e8400-e29b-41d4-a716-446655440000".to_owned(),
            description: Some("test".to_owned()),
            version: "1.2.3".to_owned(),
            url: "https://example.com".to_owned(),
            proxy_url: "https://proxy.example.com".to_owned(),
            authorization_scopes: vec!["openid".to_owned()],
            actions: vec![],
            subscriptions: None,
            dependencies: vec![],
            extensions: None,
            settings: vec![],
        }
    }

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

    #[test]
    fn validate_accepts_a_well_formed_config() {
        valid_config().validate().expect("valid config should pass");
    }

    #[test]
    fn validate_rejects_invalid_name() {
        let mut config = valid_config();
        config.name = "AB".to_owned();
        let err = config.validate().expect_err("uppercase/short name");
        assert!(err.to_string().contains("name must match"), "{err}");
    }

    #[test]
    fn is_valid_app_name_pattern() {
        assert!(is_valid_app_name("my-app"));
        assert!(is_valid_app_name("abc"));
        assert!(is_valid_app_name(&"a".repeat(255)));
        assert!(!is_valid_app_name(""));
        assert!(!is_valid_app_name("ab"));
        assert!(!is_valid_app_name("AB"));
        assert!(!is_valid_app_name("MyApp"));
        assert!(!is_valid_app_name("my_app"));
        assert!(!is_valid_app_name(&"a".repeat(256)));
    }

    #[test]
    fn validate_rejects_non_v4_uuid_client_id() {
        let mut config = valid_config();
        config.client_id = "550e8400-e29b-11d4-a716-446655440000".to_owned();
        let err = config.validate().expect_err("uuid v1");
        assert!(err.to_string().contains("client_id"), "{err}");
    }

    #[test]
    fn validate_rejects_non_semver_version() {
        let mut config = valid_config();
        config.version = "1.0".to_owned();
        let err = config.validate().expect_err("incomplete semver");
        assert!(err.to_string().contains("version"), "{err}");
    }

    #[test]
    fn validate_rejects_non_absolute_urls() {
        let mut config = valid_config();
        config.url = "/relative".to_owned();
        let err = config.validate().expect_err("relative url");
        assert!(
            err.to_string()
                .contains("url must be an absolute http(s) URL"),
            "{err}"
        );
    }

    #[test]
    fn validate_rejects_non_http_url_schemes() {
        let mut config = valid_config();
        config.url = "ftp://files.example.com/app".to_owned();
        config.proxy_url = "file:///tmp/proxy".to_owned();
        let err = config.validate().expect_err("non-http schemes");
        let msg = err.to_string();
        assert!(msg.contains("url must be an absolute http(s) URL"), "{msg}");
        assert!(
            msg.contains("proxy_url must be an absolute http(s) URL"),
            "{msg}"
        );
    }

    #[test]
    fn validate_rejects_non_http_endpoint_scheme() {
        let mut config = valid_config();
        config.actions.push(ActionConfig {
            name: "sync".to_owned(),
            url: "ftp://files.example.com/sync".to_owned(),
        });
        let err = config.validate().expect_err("ftp action endpoint");
        assert!(err.to_string().contains("actions[0].url"), "{err}");
    }

    #[test]
    fn validate_rejects_uuid_with_non_rfc4122_variant() {
        let mut config = valid_config();
        config.client_id = "550e8400-e29b-41d4-c716-446655440000".to_owned();
        let err = config.validate().expect_err("bad uuid variant");
        assert!(err.to_string().contains("client_id"), "{err}");
    }

    #[test]
    fn validate_rejects_empty_authorization_scopes() {
        let mut config = valid_config();
        config.authorization_scopes.clear();
        let err = config.validate().expect_err("empty scopes");
        assert!(err.to_string().contains("authorization_scopes"), "{err}");
    }

    #[test]
    fn validate_rejects_short_action_name_and_bad_endpoint() {
        let mut config = valid_config();
        config.actions.push(ActionConfig {
            name: "ab".to_owned(),
            url: "https://not a url".to_owned(),
        });
        let err = config.validate().expect_err("bad action");
        let msg = err.to_string();
        assert!(msg.contains("actions[0].name"), "{msg}");
        assert!(msg.contains("actions[0].url"), "{msg}");
    }

    #[test]
    fn validate_accepts_proxy_relative_action_url() {
        let mut config = valid_config();
        config.actions.push(ActionConfig {
            name: "sync".to_owned(),
            url: "/actions/sync".to_owned(),
        });
        config.validate().expect("proxy-relative action url");
    }

    #[test]
    fn validate_rejects_subscription_without_events() {
        let mut config = valid_config();
        config.subscriptions = Some(SubscriptionsConfig {
            webhook: vec![SubscriptionConfig {
                name: "hook".to_owned(),
                events: vec![],
                url: "/hooks".to_owned(),
            }],
        });
        let err = config.validate().expect_err("empty events");
        assert!(err.to_string().contains("events"), "{err}");
    }

    #[test]
    fn validate_rejects_dependency_with_bad_semver() {
        let mut config = valid_config();
        config.dependencies.push(DependenciesConfig {
            app: vec![DependencyConfig {
                name: "other-app".to_owned(),
                version: Some("not-semver".to_owned()),
            }],
            feature: vec![],
        });
        let err = config.validate().expect_err("bad dep version");
        assert!(
            err.to_string().contains("dependencies[0].app[0].version"),
            "{err}"
        );
    }

    #[test]
    fn validate_rejects_embed_without_targets() {
        let mut config = valid_config();
        config.extensions = Some(ExtensionsConfig {
            embed: vec![EmbedExtensionConfig {
                name: "panel".to_owned(),
                handle: "panel-handle".to_owned(),
                source: "ext/index.tsx".to_owned(),
                targets: vec![],
            }],
            checkout: vec![],
            blocks: None,
        });
        let err = config.validate().expect_err("missing targets");
        assert!(err.to_string().contains("targets"), "{err}");
    }

    #[test]
    fn validate_accepts_embed_with_targets() {
        let mut config = valid_config();
        config.extensions = Some(ExtensionsConfig {
            embed: vec![EmbedExtensionConfig {
                name: "panel".to_owned(),
                handle: "panel-handle".to_owned(),
                source: "ext/index.tsx".to_owned(),
                targets: vec![ExtensionTarget {
                    target: "commerce.product.details".to_owned(),
                }],
            }],
            checkout: vec![],
            blocks: None,
        });
        config.validate().expect("embed with targets should pass");
    }

    #[test]
    fn validate_rejects_empty_blocks_source() {
        let mut config = valid_config();
        config.extensions = Some(ExtensionsConfig {
            embed: vec![],
            checkout: vec![],
            blocks: Some(BlocksExtensionConfig {
                source: String::new(),
            }),
        });
        let err = config.validate().expect_err("empty blocks source");
        assert!(
            err.to_string().contains("extensions.blocks.source"),
            "{err}"
        );
    }

    #[test]
    fn validate_accepts_placement_only_setting() {
        let mut config = valid_config();
        config.settings.push(SettingConfig {
            group: "tax-center".to_owned(),
            slug: "godaddy-tax".to_owned(),
            title: None,
            description: None,
            entry_path: "/settings/godaddy-tax".to_owned(),
            order: None,
            capabilities: vec![],
            icon: None,
            metadata: None,
            presentation_file: None,
            presentation: None,
        });
        config
            .validate()
            .expect("placement-only setting should be valid");
    }

    #[test]
    fn validate_rejects_invalid_setting_slug() {
        let mut config = valid_config();
        config.settings.push(SettingConfig {
            group: "Tax_Center".to_owned(),
            slug: "godaddy-tax".to_owned(),
            title: None,
            description: None,
            entry_path: "/settings/godaddy-tax".to_owned(),
            order: None,
            capabilities: vec![],
            icon: None,
            metadata: None,
            presentation_file: None,
            presentation: None,
        });
        let err = config.validate().expect_err("bad group slug");
        assert!(err.to_string().contains("settings[0].group"), "{err}");
    }

    #[test]
    fn setting_with_presentation_round_trips_through_toml() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("godaddy.toml");
        let mut config = valid_config();
        config.settings.push(SettingConfig {
            group: "tax-center".to_owned(),
            slug: "godaddy-tax".to_owned(),
            title: Some("GoDaddy Tax".to_owned()),
            description: None,
            entry_path: "/settings/godaddy-tax".to_owned(),
            order: Some(10),
            capabilities: vec!["read".to_owned(), "write".to_owned()],
            icon: Some(SettingIcon {
                name: "percent".to_owned(),
                library: "lucide".to_owned(),
            }),
            metadata: None,
            presentation_file: None,
            presentation: Some(SettingsFormV1Presentation {
                sections: vec![SettingsFormV1Section {
                    key: "defaults".to_owned(),
                    label: "Defaults".to_owned(),
                    description: None,
                    visible_when: None,
                    fields: vec![SettingsFormV1Field::Boolean {
                        key: "autoCalculate".to_owned(),
                        label: "Auto-calculate".to_owned(),
                        description: None,
                        required: false,
                        default_value: Some(true),
                    }],
                }],
            }),
        });
        write_config(&path, &config).expect("write config with setting");
        let read_back = read_config(&path).expect("read config with setting");
        assert_eq!(read_back.settings.len(), 1);
        assert_eq!(read_back.settings[0].entry_path, "/settings/godaddy-tax");
        let SettingsFormV1Field::Boolean { default_value, .. } = &read_back.settings[0]
            .presentation
            .as_ref()
            .expect("presentation")
            .sections[0]
            .fields[0]
        else {
            unreachable!("expected boolean field");
        };
        assert_eq!(default_value, &Some(true));
    }

    #[test]
    fn setting_with_presentation_file_round_trips_without_expansion() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("godaddy.toml");
        let mut config = valid_config();
        config.settings.push(SettingConfig {
            group: "tax-center".to_owned(),
            slug: "manual-tax".to_owned(),
            title: None,
            description: None,
            entry_path: "/settings/manual-tax".to_owned(),
            order: None,
            capabilities: vec![],
            icon: None,
            metadata: None,
            presentation_file: Some("fixtures/manual-tax-presentation.json".to_owned()),
            presentation: None,
        });
        write_config(&path, &config).expect("write config with presentationFile");
        let read_back = read_config(&path).expect("read config with presentationFile");
        assert_eq!(
            read_back.settings[0].presentation_file,
            Some("fixtures/manual-tax-presentation.json".to_owned())
        );
        assert!(read_back.settings[0].presentation.is_none());
    }

    #[test]
    fn read_config_runs_validation() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("godaddy.toml");
        let mut config = valid_config();
        config.name = "x".to_owned();
        let raw = toml::to_string_pretty(&config).expect("serialize");
        std::fs::write(&path, raw).expect("write");
        let err = read_config(&path).expect_err("short name should fail validation on read");
        assert!(matches!(err, ConfigError::Validation(_)), "got {err:?}");
    }

    #[test]
    fn write_config_runs_validation() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("godaddy.toml");
        let mut config = valid_config();
        config.name = "x".to_owned();
        let err = write_config(&path, &config).expect_err("short name should fail on write");
        assert!(matches!(err, ConfigError::Validation(_)), "got {err:?}");
        assert!(!path.exists(), "invalid config must not be written");
    }
}
