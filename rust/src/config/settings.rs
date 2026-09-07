//! `[[settings]]` — placement metadata for an application-settings capability
//! registered with `app-registry-api`'s `createRelease.settings`.

use serde::{Deserialize, Serialize};

use super::settings_form::{SettingPresentation, validate_presentation};

const ALLOWED_CAPABILITIES: &[&str] = &["read", "write", "validate", "test", "delete", "open"];
const ALLOWED_ICON_LIBRARIES: &[&str] = &["ux", "lucide", "commerce"];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingConfig {
    pub group: String,
    pub slug: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    pub entry_path: String,
    #[serde(default)]
    pub order: Option<i64>,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub icon: Option<SettingIcon>,
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
    /// Path to a JSON presentation file, resolved against the manifest's
    /// directory at release time. Mutually exclusive with `presentation`.
    #[serde(default)]
    pub presentation_file: Option<String>,
    /// The `settings-form-v1` or `settings-link-v1` shape; `None` until
    /// hand-added — `release` rejects a settings entry with no presentation.
    #[serde(default)]
    pub presentation: Option<SettingPresentation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingIcon {
    pub name: String,
    pub library: String,
}

/// True when `value` matches the API's slug pattern: `^[a-z0-9]+(-[a-z0-9]+)*$`.
fn is_valid_slug(value: &str) -> bool {
    !value.is_empty()
        && value.split('-').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        })
}

/// True when `path` is a route-safe entry path: starts with `/`, no scheme,
/// query string, fragment, or `..` segments, and every non-empty segment
/// uses only `[A-Za-z0-9._~-]`.
fn is_valid_entry_path(path: &str) -> bool {
    if !path.starts_with('/') || path.contains("://") || path.contains('?') || path.contains('#') {
        return false;
    }
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        return false;
    }
    trimmed[1..].split('/').all(|segment| {
        !segment.is_empty()
            && segment != ".."
            && segment
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'~' | b'-'))
    })
}

/// Normalize an entry path the same way the API does before comparing for
/// overlap: strip trailing slashes, collapsing an all-slash path to `/`.
fn normalize_entry_path(path: &str) -> &str {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() { "/" } else { trimmed }
}

/// Port of `applicationSettingEntryPathsOverlap` (`application-setting.ts`):
/// two entry paths overlap if they're equal, one is `/`, or one is a
/// slash-bounded prefix of the other.
fn entry_paths_overlap(first: &str, second: &str) -> bool {
    let first = normalize_entry_path(first);
    let second = normalize_entry_path(second);
    if first == "/" || second == "/" {
        return true;
    }
    first == second
        || first.starts_with(&format!("{second}/"))
        || second.starts_with(&format!("{first}/"))
}

pub(super) fn validate_settings(settings: &[SettingConfig], errors: &mut Vec<String>) {
    for (i, setting) in settings.iter().enumerate() {
        let path = format!("settings[{i}]");

        if !is_valid_slug(&setting.group) {
            errors.push(format!(
                "{path}.group must match /^[a-z0-9]+(-[a-z0-9]+)*$/ (got {:?})",
                setting.group
            ));
        }
        if !is_valid_slug(&setting.slug) {
            errors.push(format!(
                "{path}.slug must match /^[a-z0-9]+(-[a-z0-9]+)*$/ (got {:?})",
                setting.slug
            ));
        }
        if !is_valid_entry_path(&setting.entry_path) {
            errors.push(format!(
                "{path}.entryPath must be a route-safe path starting with / (got {:?})",
                setting.entry_path
            ));
        }
        for capability in &setting.capabilities {
            if !ALLOWED_CAPABILITIES.contains(&capability.as_str()) {
                errors.push(format!(
                    "{path}.capabilities contains {capability:?}, must be one of {ALLOWED_CAPABILITIES:?}"
                ));
            }
        }
        if let Some(icon) = &setting.icon
            && !ALLOWED_ICON_LIBRARIES.contains(&icon.library.as_str())
        {
            errors.push(format!(
                "{path}.icon.library must be one of {ALLOWED_ICON_LIBRARIES:?} (got {:?})",
                icon.library
            ));
        }
        if setting.presentation.is_some() && setting.presentation_file.is_some() {
            errors.push(format!(
                "{path} has both presentation and presentationFile — provide only one"
            ));
        }
        if let Some(presentation) = &setting.presentation {
            validate_presentation(
                presentation,
                &setting.capabilities,
                errors,
                &format!("{path}.presentation"),
            );
        }
    }

    for i in 0..settings.len() {
        for j in (i + 1)..settings.len() {
            if entry_paths_overlap(&settings[i].entry_path, &settings[j].entry_path) {
                errors.push(format!(
                    "settings[{j}].entryPath {:?} overlaps with settings[{i}].entryPath {:?}",
                    settings[j].entry_path, settings[i].entry_path
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::settings_form::{SettingsFormV1Presentation, SettingsLinkV1Presentation};

    fn setting(slug: &str, entry_path: &str) -> SettingConfig {
        SettingConfig {
            group: "tax-center".to_owned(),
            slug: slug.to_owned(),
            title: None,
            description: None,
            entry_path: entry_path.to_owned(),
            order: None,
            capabilities: vec![],
            icon: None,
            metadata: None,
            presentation_file: None,
            presentation: None,
        }
    }

    #[test]
    fn is_valid_slug_pattern() {
        assert!(is_valid_slug("tax-center"));
        assert!(is_valid_slug("a"));
        assert!(!is_valid_slug(""));
        assert!(!is_valid_slug("Tax-Center"));
        assert!(!is_valid_slug("tax_center"));
        assert!(!is_valid_slug("-tax"));
        assert!(!is_valid_slug("tax-"));
    }

    #[test]
    fn is_valid_entry_path_shape() {
        assert!(is_valid_entry_path("/settings/manual-tax"));
        assert!(!is_valid_entry_path("settings/manual-tax"));
        assert!(!is_valid_entry_path("/settings?x=1"));
        assert!(!is_valid_entry_path("/settings#frag"));
        assert!(!is_valid_entry_path("https://example.com/settings"));
        assert!(!is_valid_entry_path("/../settings"));
        assert!(!is_valid_entry_path("/"));
    }

    #[test]
    fn entry_paths_overlap_detects_prefix_and_exact() {
        assert!(entry_paths_overlap("/settings/tax", "/settings/tax"));
        assert!(entry_paths_overlap("/settings", "/settings/tax"));
        assert!(entry_paths_overlap("/settings/tax/", "/settings/tax"));
        assert!(!entry_paths_overlap("/settings/tax", "/settings/shipping"));
    }

    #[test]
    fn validate_settings_accepts_well_formed_placement_only_entry() {
        let mut errors = Vec::new();
        validate_settings(
            &[setting("godaddy-tax", "/settings/godaddy-tax")],
            &mut errors,
        );
        assert!(errors.is_empty(), "{errors:?}");
    }

    #[test]
    fn validate_settings_rejects_invalid_capability() {
        let mut s = setting("godaddy-tax", "/settings/godaddy-tax");
        s.capabilities = vec!["read".to_owned(), "not-a-capability".to_owned()];
        let mut errors = Vec::new();
        validate_settings(&[s], &mut errors);
        assert!(
            errors.iter().any(|e| e.contains("capabilities")),
            "{errors:?}"
        );
    }

    #[test]
    fn validate_settings_rejects_invalid_icon_library() {
        let mut s = setting("godaddy-tax", "/settings/godaddy-tax");
        s.icon = Some(SettingIcon {
            name: "percent".to_owned(),
            library: "material".to_owned(),
        });
        let mut errors = Vec::new();
        validate_settings(&[s], &mut errors);
        assert!(
            errors.iter().any(|e| e.contains("icon.library")),
            "{errors:?}"
        );
    }

    #[test]
    fn validate_settings_rejects_both_presentation_and_presentation_file() {
        let mut s = setting("godaddy-tax", "/settings/godaddy-tax");
        s.presentation_file = Some("presentation.json".to_owned());
        s.presentation = Some(SettingPresentation::Form(SettingsFormV1Presentation {
            sections: vec![],
        }));
        let mut errors = Vec::new();
        validate_settings(&[s], &mut errors);
        assert!(
            errors
                .iter()
                .any(|e| e.contains("presentation") && e.contains("presentationFile")),
            "{errors:?}"
        );
    }

    #[test]
    fn validate_settings_accepts_well_formed_link_setting() {
        let mut s = setting("paypal-payments", "/settings/paypal");
        s.capabilities = vec!["read".to_owned(), "open".to_owned()];
        s.presentation = Some(SettingPresentation::Link(SettingsLinkV1Presentation {
            label: "Configure PayPal".to_owned(),
            open_mode: "new-window".to_owned(),
        }));
        let mut errors = Vec::new();
        validate_settings(&[s], &mut errors);
        assert!(errors.is_empty(), "{errors:?}");
    }

    #[test]
    fn validate_settings_rejects_link_setting_with_wrong_capabilities() {
        let mut s = setting("paypal-payments", "/settings/paypal");
        s.capabilities = vec!["read".to_owned(), "write".to_owned()];
        s.presentation = Some(SettingPresentation::Link(SettingsLinkV1Presentation {
            label: "Configure PayPal".to_owned(),
            open_mode: "new-window".to_owned(),
        }));
        let mut errors = Vec::new();
        validate_settings(&[s], &mut errors);
        assert!(
            errors
                .iter()
                .any(|e| e.contains("requires exactly the read and open capabilities")),
            "{errors:?}"
        );
    }

    #[test]
    fn validate_settings_rejects_form_setting_with_open_capability() {
        let mut s = setting("godaddy-tax", "/settings/godaddy-tax");
        s.capabilities = vec!["read".to_owned(), "open".to_owned()];
        s.presentation = Some(SettingPresentation::Form(SettingsFormV1Presentation {
            sections: vec![],
        }));
        let mut errors = Vec::new();
        validate_settings(&[s], &mut errors);
        assert!(
            errors
                .iter()
                .any(|e| e.contains("only valid for a settings-link-v1")),
            "{errors:?}"
        );
    }

    #[test]
    fn validate_settings_rejects_overlapping_entry_paths() {
        let mut errors = Vec::new();
        validate_settings(
            &[setting("a", "/settings/tax"), setting("b", "/settings/tax")],
            &mut errors,
        );
        assert!(
            errors.iter().any(|e| e.contains("overlaps with")),
            "{errors:?}"
        );
    }
}
