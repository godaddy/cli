//! Native-extension input for `gddy platform app release`, gated by the
//! `native-apps` feature flag.

use serde_json::{Value, json};

use crate::platform::app::commands::NATIVE_APPS_FLAG_KEY;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct NativeExtensionDraft {
    name: String,
    support_contact: String,
    android_package_name: String,
}

/// Resolve toml-owned native-extension fields for `ensureNativeAppDraft`.
///
/// `name` uses `[native_extension].name` when it is present and non-empty;
/// otherwise it falls back to top-level `config.name`.
fn native_extension_draft(config: &crate::config::Config) -> Option<NativeExtensionDraft> {
    let ext = config.native_extension.as_ref()?;
    let name = ext
        .name
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or(config.name.as_str())
        .to_owned();
    Some(NativeExtensionDraft {
        name,
        support_contact: ext.support_contact.clone(),
        android_package_name: ext.android_package_name.clone(),
    })
}

/// Draft for `createRelease` when the `native-apps` flag is visible.
///
/// A hidden flag yields `None` even if `[native_extension]` is present, so
/// `apply_native_extensions` omits `nativeExtensions` and skips the mix check.
pub(super) fn native_extension_draft_if_visible(
    config: &crate::config::Config,
    policy: &cli_engine::FlagPolicy,
) -> Option<NativeExtensionDraft> {
    if !policy.visible(Some(NATIVE_APPS_FLAG_KEY), cli_engine::Stage::Experimental) {
        return None;
    }
    native_extension_draft(config)
}

/// Build the `createRelease` `nativeExtensions` entry from toml-owned fields.
///
/// `platform` is required (`ANDROID` is the only `NativeExtensionPlatform`
/// variant). Categories are portal-owned and are not part of this input.
/// Unlike core `createGpaRelease`, this includes `packageName` from toml.
fn native_extensions_input(draft: &NativeExtensionDraft) -> Value {
    json!([{
        "platform": "ANDROID",
        "name": draft.name,
        "contact": draft.support_contact,
        "packageName": draft.android_package_name,
    }])
}

/// Attach toml native-extension fields to the GraphQL `createRelease` input.
///
/// Registry rejects mixing non-empty `uiExtensions` with `nativeExtensions`
/// (`HYBRID_EXTENSIONS_NOT_ALLOWED`). Empty `uiExtensions: []` is allowed.
pub(super) fn apply_native_extensions(
    input: &mut Value,
    ui_extensions: &[Value],
    draft: Option<&NativeExtensionDraft>,
) -> cli_engine::Result<()> {
    if draft.is_some() && !ui_extensions.is_empty() {
        return Err(crate::error::GddyError::validation(
            "a release cannot mix uiExtensions and a native extension",
        )
        .into_cli_error());
    }
    if let Some(draft) = draft {
        input["nativeExtensions"] = native_extensions_input(draft);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use cli_engine::{FlagPolicy, Stage};

    fn valid_release_config() -> crate::config::Config {
        crate::config::Config {
            name: "my-app".to_owned(),
            client_id: "550e8400-e29b-41d4-a716-446655440000".to_owned(),
            description: Some("test".to_owned()),
            version: "1.2.3".to_owned(),
            url: "https://example.com".to_owned(),
            proxy_url: "https://proxy.example.com".to_owned(),
            authorization_scopes: vec!["openid".to_owned()],
            redirect_uris: None,
            actions: vec![],
            subscriptions: None,
            dependencies: vec![],
            extensions: None,
            settings: vec![],
            native_extension: None,
        }
    }

    fn config_with_native_extension() -> crate::config::Config {
        let mut config = valid_release_config();
        config.native_extension = Some(crate::config::NativeExtensionConfig {
            name: Some("My Display Name".to_owned()),
            support_contact: "support@example.com".to_owned(),
            android_package_name: "com.example.app".to_owned(),
        });
        config
    }

    #[test]
    fn native_apps_flag_key_is_native_apps() {
        assert_eq!(super::NATIVE_APPS_FLAG_KEY, "native-apps");
    }

    #[test]
    fn native_extension_draft_if_visible_is_none_at_ga_when_section_present() {
        let config = config_with_native_extension();
        let policy = FlagPolicy::new();
        let draft = super::native_extension_draft_if_visible(&config, &policy);
        assert!(draft.is_none());

        let mut input = serde_json::json!({ "applicationId": "app-123", "version": "1.0.11" });
        super::apply_native_extensions(&mut input, &[], draft.as_ref())
            .expect("non-native release");
        assert!(input.get("nativeExtensions").is_none());
    }

    #[test]
    fn native_extension_draft_if_visible_is_some_when_min_stage_is_experimental() {
        let config = config_with_native_extension();
        let policy = FlagPolicy::new().with_min_stage(Stage::Experimental);
        let draft = super::native_extension_draft_if_visible(&config, &policy).expect("draft");
        assert_eq!(draft.name, "My Display Name");
        assert_eq!(draft.support_contact, "support@example.com");
        assert_eq!(draft.android_package_name, "com.example.app");

        let mut input = serde_json::json!({ "applicationId": "app-123", "version": "1.0.11" });
        super::apply_native_extensions(&mut input, &[], Some(&draft)).expect("native release");
        assert_eq!(
            input["nativeExtensions"][0]["packageName"],
            "com.example.app"
        );
    }

    #[test]
    fn native_extension_draft_if_visible_is_some_when_override_promotes_key_to_ga() {
        let config = config_with_native_extension();
        let policy = FlagPolicy::new()
            .with_min_stage(Stage::Ga)
            .with_override(super::NATIVE_APPS_FLAG_KEY, Stage::Ga);
        let draft = super::native_extension_draft_if_visible(&config, &policy).expect("draft");
        assert_eq!(draft.android_package_name, "com.example.app");
    }

    #[test]
    fn native_extension_draft_if_visible_stays_none_when_override_is_experimental_at_ga() {
        let config = config_with_native_extension();
        let policy = FlagPolicy::new()
            .with_min_stage(Stage::Ga)
            .with_override(super::NATIVE_APPS_FLAG_KEY, Stage::Experimental);
        assert!(super::native_extension_draft_if_visible(&config, &policy).is_none());
    }

    #[test]
    fn native_extension_draft_if_visible_ignores_native_section_beside_ui_extensions_at_ga() {
        let config = config_with_native_extension();
        let policy = FlagPolicy::new();
        let draft = super::native_extension_draft_if_visible(&config, &policy);
        assert!(draft.is_none());

        let mut input = serde_json::json!({ "applicationId": "app-123", "version": "1.0.11" });
        let ui = vec![serde_json::json!({ "name": "Widget", "handle": "widget" })];
        super::apply_native_extensions(&mut input, &ui, draft.as_ref()).expect("ui release");
        assert!(input.get("nativeExtensions").is_none());
        assert_eq!(ui.len(), 1);
    }

    #[test]
    fn native_extension_draft_if_visible_rejects_mix_when_flag_visible() {
        let config = config_with_native_extension();
        let policy = FlagPolicy::new().with_min_stage(Stage::Experimental);
        let draft = super::native_extension_draft_if_visible(&config, &policy).expect("draft");

        let mut input = serde_json::json!({ "applicationId": "app-123", "version": "1.0.11" });
        let ui = vec![serde_json::json!({ "name": "Widget", "handle": "widget" })];
        let err = super::apply_native_extensions(&mut input, &ui, Some(&draft))
            .expect_err("hybrid must fail locally");
        assert!(
            err.to_string().contains("cannot mix uiExtensions"),
            "got: {err}"
        );
        assert!(input.get("nativeExtensions").is_none());
    }

    #[test]
    fn native_extension_draft_if_visible_is_none_without_section_for_either_policy() {
        let config = valid_release_config();
        assert!(super::native_extension_draft_if_visible(&config, &FlagPolicy::new()).is_none());
        assert!(
            super::native_extension_draft_if_visible(
                &config,
                &FlagPolicy::new().with_min_stage(Stage::Experimental),
            )
            .is_none()
        );
    }

    #[test]
    fn native_extension_draft_is_none_when_section_absent() {
        let config = valid_release_config();
        assert!(super::native_extension_draft(&config).is_none());
    }

    #[test]
    fn native_extension_draft_falls_back_to_config_name_when_name_absent() {
        let mut config = valid_release_config();
        config.native_extension = Some(crate::config::NativeExtensionConfig {
            name: None,
            support_contact: "support@example.com".to_owned(),
            android_package_name: "com.example.app".to_owned(),
        });
        let draft = super::native_extension_draft(&config).expect("draft");
        assert_eq!(draft.name, "my-app");
        assert_eq!(draft.support_contact, "support@example.com");
        assert_eq!(draft.android_package_name, "com.example.app");
    }

    #[test]
    fn native_extension_draft_falls_back_to_config_name_when_name_empty() {
        let mut config = valid_release_config();
        config.native_extension = Some(crate::config::NativeExtensionConfig {
            name: Some(String::new()),
            support_contact: "support@example.com".to_owned(),
            android_package_name: "com.example.app".to_owned(),
        });
        let draft = super::native_extension_draft(&config).expect("draft");
        assert_eq!(draft.name, "my-app");
    }

    #[test]
    fn native_extension_draft_uses_explicit_name_when_present() {
        let mut config = valid_release_config();
        config.native_extension = Some(crate::config::NativeExtensionConfig {
            name: Some("My Display Name".to_owned()),
            support_contact: "support@example.com".to_owned(),
            android_package_name: "com.example.app".to_owned(),
        });
        let draft = super::native_extension_draft(&config).expect("draft");
        assert_eq!(draft.name, "My Display Name");
    }

    #[test]
    fn native_extensions_input_carries_every_toml_field_and_android_platform() {
        let draft = super::NativeExtensionDraft {
            name: "My Display Name".to_owned(),
            support_contact: "support@example.com".to_owned(),
            android_package_name: "com.example.app".to_owned(),
        };

        let actual = super::native_extensions_input(&draft);

        let entries = actual.as_array().expect("one-element array");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["platform"], "ANDROID");
        assert_eq!(entries[0]["name"], "My Display Name");
        assert_eq!(entries[0]["contact"], "support@example.com");
        assert_eq!(entries[0]["packageName"], "com.example.app");
        // Categories are portal-owned and never travel on createRelease.
        assert!(entries[0].get("appCategory").is_none());
        assert!(entries[0].get("merchantCategory").is_none());
    }

    fn sample_draft() -> super::NativeExtensionDraft {
        super::NativeExtensionDraft {
            name: "My Display Name".to_owned(),
            support_contact: "support@example.com".to_owned(),
            android_package_name: "com.example.app".to_owned(),
        }
    }

    #[test]
    fn apply_native_extensions_sets_create_release_input_when_draft_present() {
        let mut input = serde_json::json!({ "applicationId": "app-123", "version": "1.0.11" });
        super::apply_native_extensions(&mut input, &[], Some(&sample_draft()))
            .expect("native-only release");
        let row = &input["nativeExtensions"][0];
        assert_eq!(row["platform"], "ANDROID");
        assert_eq!(row["name"], "My Display Name");
        assert_eq!(row["contact"], "support@example.com");
        assert_eq!(row["packageName"], "com.example.app");
    }

    #[test]
    fn apply_native_extensions_omits_key_when_draft_absent() {
        let mut input = serde_json::json!({ "applicationId": "app-123", "version": "1.0.11" });
        super::apply_native_extensions(&mut input, &[], None).expect("non-native release");
        assert!(input.get("nativeExtensions").is_none());
    }

    #[test]
    fn apply_native_extensions_rejects_nonempty_ui_extensions() {
        let mut input = serde_json::json!({ "applicationId": "app-123", "version": "1.0.11" });
        let ui = vec![serde_json::json!({ "name": "Widget", "handle": "widget" })];
        let err = super::apply_native_extensions(&mut input, &ui, Some(&sample_draft()))
            .expect_err("hybrid must fail locally");
        assert!(
            err.to_string().contains("cannot mix uiExtensions"),
            "got: {err}"
        );
        assert!(input.get("nativeExtensions").is_none());
    }
}
