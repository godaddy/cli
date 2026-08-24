//! `gddy platform app release` — tag a new versioned release.

use std::path::Path;

use cli_engine::{CommandResult, CommandSpec, NextActionParam, RuntimeCommandSpec, Tier};
use serde_json::{Value, json};

use super::schemas::ApplicationRelease;
use crate::config::settings_form::{
    SettingsFormV1Presentation, presentation_from_json, validate_presentation,
};
use crate::next_action::next_action;
use crate::scopes::{APP_REGISTRY_READ, APP_REGISTRY_WRITE};

/// Build one `uiExtensions` release entry, enforcing the API's one-target-per-
/// extension limit. `target` is omitted when the extension has no targets.
fn ui_extension_entry(
    name: &str,
    handle: &str,
    source: &str,
    kind: &str,
    targets: &[crate::config::ExtensionTarget],
) -> cli_engine::Result<Value> {
    if targets.len() > 1 {
        return Err(cli_engine::CliCoreError::message(format!(
            "UI extension '{name}' has {} targets, but only one target is supported per extension during release",
            targets.len()
        )));
    }
    let mut entry = json!({ "name": name, "handle": handle, "source": source, "type": kind });
    if let Some(t) = targets.first() {
        entry["target"] = json!(t.target);
    }
    Ok(entry)
}

/// Resolves a setting's presentation from `presentation` or `presentationFile`.
fn resolve_presentation(
    setting: &crate::config::SettingConfig,
    manifest_dir: &Path,
) -> cli_engine::Result<SettingsFormV1Presentation> {
    match (&setting.presentation, &setting.presentation_file) {
        (Some(_), Some(_)) => Err(crate::error::GddyError::validation(format!(
            "setting '{}' has both presentation and presentationFile — provide only one",
            setting.slug
        ))
        .into_cli_error()),
        (Some(p), None) => Ok(p.clone()),
        (None, Some(file)) => {
            let path = manifest_dir.join(file);
            let content = std::fs::read_to_string(&path).map_err(|e| {
                crate::error::GddyError::validation(format!(
                    "setting '{}' presentationFile {} could not be read: {e}",
                    setting.slug,
                    path.display()
                ))
                .into_cli_error()
            })?;
            presentation_from_json(&content).map_err(|e| {
                crate::error::GddyError::validation(format!(
                    "setting '{}' presentationFile {} is invalid: {e}",
                    setting.slug,
                    path.display()
                ))
                .into_cli_error()
            })
        }
        (None, None) => Err(crate::error::GddyError::validation(format!(
            "setting '{}' has no presentation — add a [settings.presentation] block or a presentationFile before releasing",
            setting.slug
        ))
        .into_cli_error()),
    }
}

/// Build one `settings` release entry from a placement-only `[[settings]]`
/// block plus its presentation (inline or file-sourced).
fn setting_entry(
    setting: &crate::config::SettingConfig,
    manifest_dir: &Path,
) -> cli_engine::Result<Value> {
    let presentation = resolve_presentation(setting, manifest_dir)?;
    let mut errors = Vec::new();
    validate_presentation(&presentation, &mut errors, "presentation");
    if !errors.is_empty() {
        return Err(crate::error::GddyError::validation(format!(
            "setting '{}' presentation is invalid: {}",
            setting.slug,
            errors.join("; ")
        ))
        .into_cli_error());
    }
    let mut presentation_json = serde_json::to_value(&presentation)
        .map_err(|e| cli_engine::CliCoreError::message(e.to_string()))?;
    if let Value::Object(map) = &mut presentation_json {
        map.insert("type".to_owned(), json!("form"));
        map.insert("schemaVersion".to_owned(), json!("settings-form-v1"));
    }

    let mut entry = json!({
        "groupSlug": setting.group,
        "appSettingSlug": setting.slug,
        "entryPath": setting.entry_path,
        "presentation": presentation_json,
    });
    if let Some(title) = &setting.title {
        entry["title"] = json!(title);
    }
    if let Some(description) = &setting.description {
        entry["description"] = json!(description);
    }
    if let Some(icon) = &setting.icon {
        entry["iconName"] = json!(icon.name);
        entry["iconLibrary"] = json!(icon.library);
    }
    if let Some(order) = setting.order {
        entry["order"] = json!(order);
    }
    if !setting.capabilities.is_empty() {
        entry["capabilities"] = json!(setting.capabilities);
    }
    if let Some(metadata) = &setting.metadata {
        entry["metadata"] = metadata.clone();
    }
    Ok(entry)
}

/// Map godaddy.toml `[[settings]]` placements to the release `settings` input.
fn build_settings(
    config: &crate::config::Config,
    manifest_dir: &Path,
) -> cli_engine::Result<Vec<Value>> {
    config
        .settings
        .iter()
        .map(|s| setting_entry(s, manifest_dir))
        .collect()
}

/// Missing manifest returns `Ok(None)`; a manifest that exists but fails to
/// read, parse, or validate is an error rather than a silent empty fallback.
fn load_manifest(path: &Path) -> cli_engine::Result<Option<crate::config::Config>> {
    match crate::config::read_config(path) {
        Ok(config) => Ok(Some(config)),
        Err(crate::config::ConfigError::NotFound { path }) => {
            tracing::warn!(
                path = %path,
                "no manifest found; releasing with empty actions, subscriptions, uiExtensions, and settings"
            );
            Ok(None)
        }
        Err(e) => Err(crate::error::GddyError::config(format!(
            "failed to load {}: {e}",
            path.display()
        ))
        .into_cli_error()),
    }
}

/// Map godaddy.toml extensions (embed / checkout / blocks) to the release
/// `uiExtensions` input. Mirrors the TS release mapping (single target each).
fn build_ui_extensions(config: &crate::config::Config) -> cli_engine::Result<Vec<Value>> {
    let mut out = Vec::new();
    let Some(exts) = &config.extensions else {
        return Ok(out);
    };
    for e in &exts.embed {
        out.push(ui_extension_entry(
            &e.name, &e.handle, &e.source, "embed", &e.targets,
        )?);
    }
    for e in &exts.checkout {
        out.push(ui_extension_entry(
            &e.name, &e.handle, &e.source, "checkout", &e.targets,
        )?);
    }
    if let Some(b) = &exts.blocks {
        // Blocks carries no name/handle/targets in config; use the same fixed
        // identifiers the TS release path uses.
        out.push(
            json!({ "name": "Blocks", "handle": "blocks", "source": b.source, "type": "blocks" }),
        );
    }
    Ok(out)
}

#[derive(Debug, Clone, clap::Args)]
struct ReleaseArgs {
    /// Application ID.
    #[arg(long = "application-id", value_name = "ID")]
    application_id: String,

    /// Semver release version.
    #[arg(long, value_name = "VERSION")]
    version: String,

    /// Release description.
    #[arg(long, value_name = "TEXT")]
    description: Option<String>,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<ReleaseArgs, _, _, _>(
        CommandSpec::from_args::<ReleaseArgs>("release", "Create a new application release")
            .with_long(
                "Tag a new versioned release for a GoDaddy developer-platform \
                application. The version must follow semver (e.g. 1.2.3). A \
                release is required before running `gddy platform app deploy`. \
                Use `gddy platform app info --name <name>` to retrieve the \
                application ID.",
            )
            .with_system("applications")
            .with_tier(Tier::Mutate)
            .with_scopes(&[APP_REGISTRY_READ, APP_REGISTRY_WRITE])
            .with_output_schema::<ApplicationRelease>(),
        |ctx, args: ReleaseArgs| async move {
            let app_id = args.application_id;
            let version = args.version;
            let description = args.description;
            let mut input = json!({ "applicationId": app_id, "version": version });
            if let Some(desc) = description {
                input["description"] = json!(desc);
            }

            let config_path = crate::config::config_path(Some(&ctx.middleware.env));
            let manifest_dir = config_path.parent().unwrap_or_else(|| Path::new(""));
            // Pulls actions/subscriptions/uiExtensions/settings from godaddy.toml; see load_manifest.
            let (actions, subscriptions, ui_extensions, settings) =
                match load_manifest(&config_path)? {
                    Some(config) => {
                        let actions: Vec<Value> = config
                            .actions
                            .iter()
                            .map(|a| json!({ "name": a.name, "url": a.url }))
                            .collect();
                        let subscriptions: Vec<Value> = config
                            .subscriptions
                            .as_ref()
                            .map(|s| {
                                s.webhook
                                    .iter()
                                    .map(|w| {
                                        json!({ "name": w.name, "events": w.events, "url": w.url })
                                    })
                                    .collect()
                            })
                            .unwrap_or_default();
                        let ui_extensions = build_ui_extensions(&config)?;
                        let settings = build_settings(&config, manifest_dir)?;
                        (actions, subscriptions, ui_extensions, settings)
                    }
                    None => (Vec::new(), Vec::new(), Vec::new(), Vec::new()),
                };
            input["actions"] = json!(actions);
            input["subscriptions"] = json!(subscriptions);
            input["uiExtensions"] = json!(ui_extensions);
            input["settings"] = json!(settings);

            let client = super::make_client(&ctx).await?;
            let data = client
                .create_release(input)
                .await
                .map_err(super::client_err)?;
            // Release is keyed by `--application-id`, not name. Do not prefill
            // `name` from godaddy.toml — that manifest may belong to a different app.
            let name_param = NextActionParam::required();
            Ok(
                CommandResult::new(data["createRelease"].clone()).with_next_actions(vec![
                    next_action(
                        "platform app deploy --name <name>",
                        "Deploy the released application",
                    )
                    .with_param("name", name_param.clone()),
                    next_action(
                        "platform app info --name <name>",
                        "Inspect application and latest release",
                    )
                    .with_param("name", name_param),
                ]),
            )
        },
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn command_accepts_version_flag() {
        super::command()
            .spec
            .clap_command()
            .try_get_matches_from([
                "release",
                "--application-id",
                "app-123",
                "--version",
                "1.2.3",
            ])
            .expect("release version flag should be accepted");
    }

    #[test]
    fn ui_extension_entry_maps_fields_and_target() {
        use crate::config::ExtensionTarget;

        // No targets: `target` is omitted.
        let none = super::ui_extension_entry("Widget", "widget", "src/w.ts", "embed", &[])
            .expect("entry builds");
        assert_eq!(none["name"], "Widget");
        assert_eq!(none["handle"], "widget");
        assert_eq!(none["type"], "embed");
        assert_eq!(none["source"], "src/w.ts");
        assert!(none.get("target").is_none());

        // Exactly one target: `target` is set to that value.
        let one = super::ui_extension_entry(
            "Widget",
            "widget",
            "src/w.ts",
            "checkout",
            &[ExtensionTarget {
                target: "checkout.block".to_owned(),
            }],
        )
        .expect("entry builds");
        assert_eq!(one["target"], "checkout.block");
    }

    fn placement_only_setting() -> crate::config::SettingConfig {
        crate::config::SettingConfig {
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
        }
    }

    fn boolean_presentation() -> crate::config::settings_form::SettingsFormV1Presentation {
        use crate::config::settings_form::{SettingsFormV1Field, SettingsFormV1Section};
        crate::config::settings_form::SettingsFormV1Presentation {
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
        }
    }

    fn list_group_presentation() -> crate::config::settings_form::SettingsFormV1Presentation {
        use crate::config::settings_form::{
            ChoiceOption, ListGroupItem, SelectValue, SettingsFormV1Field, SettingsFormV1Section,
        };
        crate::config::settings_form::SettingsFormV1Presentation {
            sections: vec![SettingsFormV1Section {
                key: "rules".to_owned(),
                label: "Rules".to_owned(),
                description: None,
                visible_when: None,
                fields: vec![SettingsFormV1Field::ListGroup {
                    key: "rules".to_owned(),
                    label: "Rules".to_owned(),
                    description: None,
                    required: false,
                    min_items: None,
                    max_items: None,
                    item: ListGroupItem {
                        id_field: "id".to_owned(),
                        title_field: None,
                        fields: vec![SettingsFormV1Field::Select {
                            key: "country".to_owned(),
                            label: "Country".to_owned(),
                            description: None,
                            required: true,
                            options: vec![ChoiceOption {
                                value: SelectValue::Str("US".to_owned()),
                                label: "United States".to_owned(),
                                description: None,
                            }],
                            default_value: None,
                        }],
                    },
                }],
            }],
        }
    }

    #[test]
    fn setting_entry_rejects_missing_presentation() {
        let err = super::setting_entry(&placement_only_setting(), std::path::Path::new(""))
            .expect_err("missing presentation must be rejected");
        assert!(err.to_string().contains("no presentation"), "{err}");
    }

    #[test]
    fn setting_entry_maps_placement_and_presentation() {
        let mut setting = placement_only_setting();
        setting.presentation = Some(boolean_presentation());
        let entry = super::setting_entry(&setting, std::path::Path::new("")).expect("entry builds");
        assert_eq!(entry["groupSlug"], "tax-center");
        assert_eq!(entry["appSettingSlug"], "godaddy-tax");
        assert_eq!(entry["entryPath"], "/settings/godaddy-tax");
        assert_eq!(entry["presentation"]["type"], "form");
        assert_eq!(entry["presentation"]["schemaVersion"], "settings-form-v1");
        assert_eq!(
            entry["presentation"]["sections"][0]["fields"][0]["type"],
            "boolean"
        );
        assert!(
            entry.get("capabilities").is_none(),
            "empty capabilities should be omitted"
        );
        assert!(
            entry.get("iconName").is_none(),
            "absent icon should be omitted"
        );
        assert!(
            entry["presentation"]["sections"][0]
                .get("visibleWhen")
                .is_none(),
            "absent section visibility should be omitted instead of serialized as null"
        );
        assert!(
            entry["presentation"]["sections"][0]["fields"][0]
                .get("description")
                .is_none(),
            "absent field properties should be omitted instead of serialized as null"
        );
    }

    #[test]
    fn setting_entry_includes_optional_fields_when_present() {
        let mut setting = placement_only_setting();
        setting.presentation = Some(boolean_presentation());
        setting.title = Some("GoDaddy Tax".to_owned());
        setting.description = Some("Tax settings".to_owned());
        setting.order = Some(10);
        setting.capabilities = vec!["read".to_owned(), "write".to_owned()];
        setting.icon = Some(crate::config::SettingIcon {
            name: "percent".to_owned(),
            library: "lucide".to_owned(),
        });
        setting.metadata = Some(serde_json::json!({ "provider": "godaddy-tax" }));
        let entry = super::setting_entry(&setting, std::path::Path::new("")).expect("entry builds");
        assert_eq!(entry["title"], "GoDaddy Tax");
        assert_eq!(entry["description"], "Tax settings");
        assert_eq!(entry["order"], 10);
        assert_eq!(entry["capabilities"], serde_json::json!(["read", "write"]));
        assert_eq!(entry["iconName"], "percent");
        assert_eq!(entry["iconLibrary"], "lucide");
        assert_eq!(entry["metadata"]["provider"], "godaddy-tax");
    }

    #[test]
    fn setting_entry_omits_absent_list_group_and_choice_option_fields() {
        let mut setting = placement_only_setting();
        setting.presentation = Some(list_group_presentation());

        let entry = super::setting_entry(&setting, std::path::Path::new(""))
            .expect("list-group entry builds");
        let list_group = &entry["presentation"]["sections"][0]["fields"][0];
        assert!(
            list_group.get("minItems").is_none(),
            "absent list-group bounds should be omitted instead of serialized as null"
        );
        assert!(
            list_group["item"].get("titleField").is_none(),
            "absent list-group item properties should be omitted instead of serialized as null"
        );

        let option = &list_group["item"]["fields"][0]["options"][0];
        assert!(
            option.get("description").is_none(),
            "absent choice-option properties should be omitted instead of serialized as null"
        );
    }

    #[test]
    fn setting_entry_resolves_presentation_file_relative_to_manifest_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("presentation.json"),
            serde_json::json!({
                "type": "form",
                "schemaVersion": "settings-form-v1",
                "sections": [{
                    "key": "defaults",
                    "label": "Defaults",
                    "fields": [{
                        "type": "boolean",
                        "key": "autoCalculate",
                        "label": "Auto-calculate",
                        "defaultValue": true,
                    }],
                }],
            })
            .to_string(),
        )
        .expect("write presentation fixture");

        let mut setting = placement_only_setting();
        setting.presentation_file = Some("presentation.json".to_owned());
        let via_file = super::setting_entry(&setting, dir.path()).expect("entry builds from file");

        let mut inline = placement_only_setting();
        inline.presentation = Some(boolean_presentation());
        let via_inline =
            super::setting_entry(&inline, std::path::Path::new("")).expect("entry builds inline");

        assert_eq!(via_file["presentation"], via_inline["presentation"]);
    }

    #[test]
    fn setting_entry_rejects_missing_presentation_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut setting = placement_only_setting();
        setting.presentation_file = Some("missing.json".to_owned());
        let err = super::setting_entry(&setting, dir.path())
            .expect_err("missing presentation file must be rejected");
        assert!(err.to_string().contains("could not be read"), "{err}");
    }

    #[test]
    fn setting_entry_rejects_malformed_presentation_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("presentation.json"), "not json").expect("write fixture");
        let mut setting = placement_only_setting();
        setting.presentation_file = Some("presentation.json".to_owned());
        let err = super::setting_entry(&setting, dir.path())
            .expect_err("malformed JSON must be rejected");
        assert!(err.to_string().contains("is invalid"), "{err}");
    }

    #[test]
    fn setting_entry_rejects_wrong_schema_version_in_presentation_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("presentation.json"),
            serde_json::json!({
                "type": "form",
                "schemaVersion": "something-else",
                "sections": [],
            })
            .to_string(),
        )
        .expect("write fixture");
        let mut setting = placement_only_setting();
        setting.presentation_file = Some("presentation.json".to_owned());
        let err = super::setting_entry(&setting, dir.path())
            .expect_err("wrong schemaVersion must be rejected");
        assert!(err.to_string().contains("schemaVersion"), "{err}");
    }

    #[test]
    fn setting_entry_rejects_both_presentation_and_presentation_file() {
        let mut setting = placement_only_setting();
        setting.presentation = Some(boolean_presentation());
        setting.presentation_file = Some("presentation.json".to_owned());
        let err = super::setting_entry(&setting, std::path::Path::new(""))
            .expect_err("both set must be rejected");
        assert!(err.to_string().contains("presentationFile"), "{err}");
    }

    #[test]
    fn load_manifest_returns_none_for_missing_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("godaddy.toml");
        let result = super::load_manifest(&path).expect("missing manifest is not an error");
        assert!(result.is_none());
    }

    #[test]
    fn load_manifest_fails_release_on_parse_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("godaddy.toml");
        std::fs::write(&path, "this = is not [valid toml").expect("write manifest");
        let err = super::load_manifest(&path).expect_err("parse error must fail the release");
        assert!(
            err.to_string().contains("failed to load"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn load_manifest_fails_release_on_validation_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("godaddy.toml");
        // Parses cleanly; `name` fails Config::validate's pattern check.
        std::fs::write(
            &path,
            r#"
name = "Not Valid!"
client_id = "3fa85f64-5717-4562-b3fc-2c963f66afa6"
version = "1.0.0"
url = "https://example.com"
proxy_url = "https://example.com/proxy"
authorization_scopes = []
"#,
        )
        .expect("write manifest");
        let err = super::load_manifest(&path).expect_err("validation error must fail the release");
        assert!(
            err.to_string().contains("failed to load"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn ui_extension_entry_rejects_multiple_targets() {
        use crate::config::ExtensionTarget;
        let targets = vec![
            ExtensionTarget {
                target: "a".to_owned(),
            },
            ExtensionTarget {
                target: "b".to_owned(),
            },
        ];
        let err = super::ui_extension_entry("Widget", "widget", "src/w.ts", "embed", &targets)
            .expect_err("more than one target must be rejected");
        assert!(
            err.to_string().contains("only one target is supported"),
            "unexpected error: {err}"
        );
    }
}
