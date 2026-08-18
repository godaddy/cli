//! `gddy platform app release` — tag a new versioned release.

use cli_engine::{CommandResult, CommandSpec, NextActionParam, RuntimeCommandSpec, Tier};
use serde_json::{Value, json};

use super::schemas::ApplicationRelease;
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

/// Build one `settings` release entry from a placement-only `[[settings]]`
/// block plus its (required, hand-authored) `[settings.presentation]`.
fn setting_entry(setting: &crate::config::SettingConfig) -> cli_engine::Result<Value> {
    let Some(presentation) = &setting.presentation else {
        return Err(cli_engine::CliCoreError::message(format!(
            "settings '{}' has no presentation — add a [settings.presentation] block before releasing",
            setting.slug
        )));
    };
    let mut presentation_json = serde_json::to_value(presentation)
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
fn build_settings(config: &crate::config::Config) -> cli_engine::Result<Vec<Value>> {
    config.settings.iter().map(setting_entry).collect()
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
            // Include actions, webhook subscriptions, and UI extensions from
            // godaddy.toml so configured behavior is captured in the release.
            // Without this, everything added via `platform app add` was silently
            // dropped. A missing or invalid config is non-fatal (empty arrays);
            // too many targets per extension is a hard error.
            let (actions, subscriptions, ui_extensions, settings) = match crate::config::read_config(
                &config_path,
            ) {
                Ok(config) => {
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
                                .map(
                                    |w| json!({ "name": w.name, "events": w.events, "url": w.url }),
                                )
                                .collect()
                        })
                        .unwrap_or_default();
                    let ui_extensions = build_ui_extensions(&config)?;
                    let settings = build_settings(&config)?;
                    (actions, subscriptions, ui_extensions, settings)
                }
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        path = %config_path.display(),
                        "failed to read config; releasing with empty actions, subscriptions, uiExtensions, and settings"
                    );
                    (Vec::new(), Vec::new(), Vec::new(), Vec::new())
                }
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

    #[test]
    fn setting_entry_rejects_missing_presentation() {
        let err = super::setting_entry(&placement_only_setting())
            .expect_err("missing presentation must be rejected");
        assert!(err.to_string().contains("no presentation"), "{err}");
    }

    #[test]
    fn setting_entry_maps_placement_and_presentation() {
        let mut setting = placement_only_setting();
        setting.presentation = Some(boolean_presentation());
        let entry = super::setting_entry(&setting).expect("entry builds");
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
        let entry = super::setting_entry(&setting).expect("entry builds");
        assert_eq!(entry["title"], "GoDaddy Tax");
        assert_eq!(entry["description"], "Tax settings");
        assert_eq!(entry["order"], 10);
        assert_eq!(entry["capabilities"], serde_json::json!(["read", "write"]));
        assert_eq!(entry["iconName"], "percent");
        assert_eq!(entry["iconLibrary"], "lucide");
        assert_eq!(entry["metadata"]["provider"], "godaddy-tax");
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
