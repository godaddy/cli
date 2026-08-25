//! `[settings.presentation]` — the `settings-form-v1` shape registered with
//! `app-registry-api`'s `createRelease.settings[].presentation`. Structural
//! shape only; bounds/default-consistency/depth checks stay server-validated.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingsFormV1Presentation {
    pub sections: Vec<SettingsFormV1Section>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsFormV1Section {
    pub key: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible_when: Option<SettingsFormV1VisibilityCondition>,
    pub fields: Vec<SettingsFormV1Field>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingsFormV1VisibilityCondition {
    pub field: String,
    pub equals: SelectValue,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SettingsFormV1Field {
    #[serde(rename = "text", rename_all = "camelCase")]
    Text {
        key: String,
        label: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        #[serde(default)]
        required: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        placeholder: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        min_length: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max_length: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        default_value: Option<String>,
    },
    #[serde(rename = "textarea", rename_all = "camelCase")]
    Textarea {
        key: String,
        label: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        #[serde(default)]
        required: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        placeholder: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        min_length: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max_length: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        default_value: Option<String>,
    },
    #[serde(rename = "number", rename_all = "camelCase")]
    Number {
        key: String,
        label: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        #[serde(default)]
        required: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        min: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        step: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        suffix: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        default_value: Option<f64>,
    },
    #[serde(rename = "boolean", rename_all = "camelCase")]
    Boolean {
        key: String,
        label: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        #[serde(default)]
        required: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        default_value: Option<bool>,
    },
    #[serde(rename = "select", rename_all = "camelCase")]
    Select {
        key: String,
        label: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        #[serde(default)]
        required: bool,
        options: Vec<ChoiceOption>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        default_value: Option<SelectValue>,
    },
    #[serde(rename = "multi-select", rename_all = "camelCase")]
    MultiSelect {
        key: String,
        label: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        #[serde(default)]
        required: bool,
        options: Vec<ChoiceOption>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        min_items: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max_items: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        default_value: Option<Vec<SelectValue>>,
    },
    #[serde(rename = "list-group", rename_all = "camelCase")]
    ListGroup {
        key: String,
        label: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        #[serde(default)]
        required: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        min_items: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max_items: Option<u32>,
        item: ListGroupItem,
    },
}

impl SettingsFormV1Field {
    fn key(&self) -> &str {
        match self {
            Self::Text { key, .. }
            | Self::Textarea { key, .. }
            | Self::Number { key, .. }
            | Self::Boolean { key, .. }
            | Self::Select { key, .. }
            | Self::MultiSelect { key, .. }
            | Self::ListGroup { key, .. } => key,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListGroupItem {
    pub id_field: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title_field: Option<String>,
    pub fields: Vec<SettingsFormV1Field>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChoiceOption {
    pub value: SelectValue,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SelectValue {
    Str(String),
    Num(f64),
    Bool(bool),
}

/// The `settings-link-v1` shape — for a GPA that owns its own configuration
/// page or provider authorization flow instead of a native form.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsLinkV1Presentation {
    pub label: String,
    pub open_mode: String,
}

/// A setting's presentation — `sections` vs `label`+`openMode` are
/// structurally disjoint, so untagged matching is unambiguous.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SettingPresentation {
    Form(SettingsFormV1Presentation),
    Link(SettingsLinkV1Presentation),
}

/// True when `key` matches the same `fieldNamePattern` the API uses:
/// `^[A-Za-z][A-Za-z0-9_]*$`.
fn is_field_name(key: &str) -> bool {
    let mut chars = key.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Validates a presentation's own shape plus its capabilities, mirroring the
/// API's `validatePresentationCapabilities` refinement in `application-setting.ts`.
pub(crate) fn validate_presentation(
    presentation: &SettingPresentation,
    capabilities: &[String],
    errors: &mut Vec<String>,
    path: &str,
) {
    match presentation {
        SettingPresentation::Form(form) => {
            if capabilities.iter().any(|c| c == "open") {
                errors.push(format!(
                    "{path}: the \"open\" capability is only valid for a settings-link-v1 presentation"
                ));
            }
            validate_form_presentation(form, errors, path);
        }
        SettingPresentation::Link(link) => {
            let capability_set: HashSet<&str> = capabilities.iter().map(String::as_str).collect();
            if capabilities.len() != 2
                || !capability_set.contains("read")
                || !capability_set.contains("open")
            {
                errors.push(format!(
                    "{path}: a settings-link-v1 presentation requires exactly the read and open capabilities"
                ));
            }
            validate_link_presentation(link, errors, path);
        }
    }
}

/// Structural validation for a link presentation: non-empty `label`, and
/// `openMode` matching the only value the API accepts today.
fn validate_link_presentation(
    presentation: &SettingsLinkV1Presentation,
    errors: &mut Vec<String>,
    path: &str,
) {
    if presentation.label.trim().is_empty() {
        errors.push(format!("{path}.label must not be empty"));
    }
    if presentation.open_mode != "new-window" {
        errors.push(format!(
            "{path}.openMode must be \"new-window\" (got {:?})",
            presentation.open_mode
        ));
    }
}

/// Structural validation for a form block: field-name shape, non-empty
/// choice options, and unique section/field keys.
fn validate_form_presentation(
    presentation: &SettingsFormV1Presentation,
    errors: &mut Vec<String>,
    path: &str,
) {
    let mut seen_section_keys = HashSet::new();
    let mut seen_top_level_field_keys = HashSet::new();

    for (i, section) in presentation.sections.iter().enumerate() {
        let section_path = format!("{path}.sections[{i}]");
        if !is_field_name(&section.key) {
            errors.push(format!(
                "{section_path}.key must match ^[A-Za-z][A-Za-z0-9_]*$ (got {:?})",
                section.key
            ));
        }
        if !seen_section_keys.insert(section.key.clone()) {
            errors.push(format!(
                "{section_path}.key {:?} duplicates another section key",
                section.key
            ));
        }

        for (j, field) in section.fields.iter().enumerate() {
            let field_path = format!("{section_path}.fields[{j}]");
            validate_field(field, errors, &field_path);
            if !seen_top_level_field_keys.insert(field.key().to_owned()) {
                errors.push(format!(
                    "{field_path}.key {:?} duplicates another field key",
                    field.key()
                ));
            }
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FormPresentationFileDocument {
    #[serde(default)]
    schema_version: Option<String>,
    sections: Vec<SettingsFormV1Section>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LinkPresentationFileDocument {
    #[serde(default)]
    schema_version: Option<String>,
    label: String,
    open_mode: String,
}

/// Parses a `presentationFile`'s JSON, dispatching on `type` (defaults to
/// `"form"` when absent, matching existing fixtures that never wrote it).
pub(crate) fn presentation_from_json(content: &str) -> Result<SettingPresentation, String> {
    let value: serde_json::Value = serde_json::from_str(content).map_err(|e| e.to_string())?;
    let kind = match value.get("type") {
        None | Some(serde_json::Value::Null) => "form".to_owned(),
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(other) => return Err(format!("type must be a string (got {other:?})")),
    };

    match kind.as_str() {
        "form" => {
            let doc: FormPresentationFileDocument =
                serde_json::from_value(value).map_err(|e| e.to_string())?;
            if let Some(v) = &doc.schema_version
                && v != "settings-form-v1"
            {
                return Err(format!(
                    "schemaVersion must be \"settings-form-v1\" (got {v:?})"
                ));
            }
            Ok(SettingPresentation::Form(SettingsFormV1Presentation {
                sections: doc.sections,
            }))
        }
        "link" => {
            let doc: LinkPresentationFileDocument =
                serde_json::from_value(value).map_err(|e| e.to_string())?;
            if let Some(v) = &doc.schema_version
                && v != "settings-link-v1"
            {
                return Err(format!(
                    "schemaVersion must be \"settings-link-v1\" (got {v:?})"
                ));
            }
            Ok(SettingPresentation::Link(SettingsLinkV1Presentation {
                label: doc.label,
                open_mode: doc.open_mode,
            }))
        }
        other => Err(format!("type must be \"form\" or \"link\" (got {other:?})")),
    }
}

fn validate_field(field: &SettingsFormV1Field, errors: &mut Vec<String>, path: &str) {
    if !is_field_name(field.key()) {
        errors.push(format!(
            "{path}.key must match ^[A-Za-z][A-Za-z0-9_]*$ (got {:?})",
            field.key()
        ));
    }
    match field {
        SettingsFormV1Field::Select { options, .. }
        | SettingsFormV1Field::MultiSelect { options, .. } => {
            if options.is_empty() {
                errors.push(format!("{path}.options must contain at least one option"));
            }
        }
        SettingsFormV1Field::ListGroup { item, .. } => {
            if !is_field_name(&item.id_field) {
                errors.push(format!(
                    "{path}.item.idField must match ^[A-Za-z][A-Za-z0-9_]*$ (got {:?})",
                    item.id_field
                ));
            }
            if let Some(title_field) = &item.title_field
                && !is_field_name(title_field)
            {
                errors.push(format!(
                    "{path}.item.titleField must match ^[A-Za-z][A-Za-z0-9_]*$ (got {:?})",
                    title_field
                ));
            }
            for (k, inner) in item.fields.iter().enumerate() {
                validate_field(inner, errors, &format!("{path}.item.fields[{k}]"));
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_field(key: &str) -> SettingsFormV1Field {
        SettingsFormV1Field::Text {
            key: key.to_owned(),
            label: "Label".to_owned(),
            description: None,
            required: false,
            placeholder: None,
            min_length: None,
            max_length: None,
            default_value: None,
        }
    }

    fn section(key: &str, fields: Vec<SettingsFormV1Field>) -> SettingsFormV1Section {
        SettingsFormV1Section {
            key: key.to_owned(),
            label: "Label".to_owned(),
            description: None,
            visible_when: None,
            fields,
        }
    }

    #[test]
    fn is_field_name_pattern() {
        assert!(is_field_name("calculateUsing"));
        assert!(is_field_name("a"));
        assert!(is_field_name("a_1"));
        assert!(!is_field_name(""));
        assert!(!is_field_name("1abc"));
        assert!(!is_field_name("has-dash"));
    }

    fn form(sections: Vec<SettingsFormV1Section>) -> SettingPresentation {
        SettingPresentation::Form(SettingsFormV1Presentation { sections })
    }

    fn link(label: &str, open_mode: &str) -> SettingPresentation {
        SettingPresentation::Link(SettingsLinkV1Presentation {
            label: label.to_owned(),
            open_mode: open_mode.to_owned(),
        })
    }

    #[test]
    fn validate_presentation_accepts_well_formed() {
        let presentation = form(vec![section(
            "defaults",
            vec![text_field("calculateUsing")],
        )]);
        let mut errors = Vec::new();
        validate_presentation(&presentation, &[], &mut errors, "settings[0].presentation");
        assert!(errors.is_empty(), "{errors:?}");
    }

    #[test]
    fn validate_presentation_rejects_duplicate_section_keys() {
        let presentation = form(vec![
            section("defaults", vec![text_field("a")]),
            section("defaults", vec![text_field("b")]),
        ]);
        let mut errors = Vec::new();
        validate_presentation(&presentation, &[], &mut errors, "settings[0].presentation");
        assert!(
            errors
                .iter()
                .any(|e| e.contains("duplicates another section key")),
            "{errors:?}"
        );
    }

    #[test]
    fn validate_presentation_rejects_duplicate_field_keys_across_sections() {
        let presentation = form(vec![
            section("a", vec![text_field("shared")]),
            section("b", vec![text_field("shared")]),
        ]);
        let mut errors = Vec::new();
        validate_presentation(&presentation, &[], &mut errors, "settings[0].presentation");
        assert!(
            errors
                .iter()
                .any(|e| e.contains("duplicates another field key")),
            "{errors:?}"
        );
    }

    #[test]
    fn validate_presentation_rejects_bad_field_key() {
        let presentation = form(vec![section("defaults", vec![text_field("bad-key")])]);
        let mut errors = Vec::new();
        validate_presentation(&presentation, &[], &mut errors, "settings[0].presentation");
        assert!(
            errors.iter().any(|e| e.contains("must match")),
            "{errors:?}"
        );
    }

    #[test]
    fn validate_presentation_rejects_empty_select_options() {
        let field = SettingsFormV1Field::Select {
            key: "choice".to_owned(),
            label: "Choice".to_owned(),
            description: None,
            required: false,
            options: vec![],
            default_value: None,
        };
        let presentation = form(vec![section("defaults", vec![field])]);
        let mut errors = Vec::new();
        validate_presentation(&presentation, &[], &mut errors, "settings[0].presentation");
        assert!(
            errors.iter().any(|e| e.contains("options must contain")),
            "{errors:?}"
        );
    }

    #[test]
    fn validate_presentation_rejects_open_capability_on_form() {
        let presentation = form(vec![section("defaults", vec![text_field("a")])]);
        let mut errors = Vec::new();
        validate_presentation(
            &presentation,
            &["read".to_owned(), "open".to_owned()],
            &mut errors,
            "settings[0].presentation",
        );
        assert!(
            errors
                .iter()
                .any(|e| e.contains("only valid for a settings-link-v1")),
            "{errors:?}"
        );
    }

    #[test]
    fn validate_presentation_accepts_well_formed_link() {
        let presentation = link("Configure PayPal", "new-window");
        let mut errors = Vec::new();
        validate_presentation(
            &presentation,
            &["read".to_owned(), "open".to_owned()],
            &mut errors,
            "settings[0].presentation",
        );
        assert!(errors.is_empty(), "{errors:?}");
    }

    #[test]
    fn validate_presentation_rejects_link_without_exact_read_open_capabilities() {
        let presentation = link("Configure PayPal", "new-window");
        let mut errors = Vec::new();
        validate_presentation(
            &presentation,
            &["read".to_owned(), "write".to_owned()],
            &mut errors,
            "settings[0].presentation",
        );
        assert!(
            errors
                .iter()
                .any(|e| e.contains("requires exactly the read and open capabilities")),
            "{errors:?}"
        );
    }

    #[test]
    fn validate_presentation_rejects_link_with_empty_label() {
        let presentation = link("", "new-window");
        let mut errors = Vec::new();
        validate_presentation(
            &presentation,
            &["read".to_owned(), "open".to_owned()],
            &mut errors,
            "settings[0].presentation",
        );
        assert!(
            errors.iter().any(|e| e.contains("label must not be empty")),
            "{errors:?}"
        );
    }

    #[test]
    fn validate_presentation_rejects_link_with_wrong_open_mode() {
        let presentation = link("Configure PayPal", "same-window");
        let mut errors = Vec::new();
        validate_presentation(
            &presentation,
            &["read".to_owned(), "open".to_owned()],
            &mut errors,
            "settings[0].presentation",
        );
        assert!(
            errors.iter().any(|e| e.contains("openMode must be")),
            "{errors:?}"
        );
    }

    #[test]
    fn presentation_from_json_parses_link() {
        let json = r#"{
            "type": "link",
            "schemaVersion": "settings-link-v1",
            "label": "Configure PayPal",
            "openMode": "new-window"
        }"#;
        let presentation = presentation_from_json(json).expect("valid link presentation json");
        let SettingPresentation::Link(link) = presentation else {
            unreachable!("expected link presentation");
        };
        assert_eq!(link.label, "Configure PayPal");
        assert_eq!(link.open_mode, "new-window");
    }

    #[test]
    fn presentation_from_json_rejects_wrong_link_schema_version() {
        let json = r#"{
            "type": "link",
            "schemaVersion": "settings-link-v2",
            "label": "Configure PayPal",
            "openMode": "new-window"
        }"#;
        let err = presentation_from_json(json).expect_err("wrong schemaVersion must be rejected");
        assert!(err.contains("schemaVersion"), "{err}");
    }

    #[test]
    fn presentation_from_json_rejects_non_string_type() {
        let json = r#"{"type": 123, "sections": []}"#;
        let err = presentation_from_json(json).expect_err("non-string type must be rejected");
        assert!(err.contains("type must be a string"), "{err}");
    }

    #[test]
    fn toml_round_trip_preserves_nested_list_group_and_default_value() {
        let toml_src = r#"
[[sections]]
key = "defaults"
label = "Calculation defaults"

[[sections.fields]]
type = "select"
key = "calculateUsing"
label = "Calculate using"
required = true
defaultValue = "destination"

[[sections.fields.options]]
label = "Customer destination"
value = "destination"

[[sections]]
key = "rules"
label = "Rules"

[[sections.fields]]
type = "list-group"
key = "rules"
label = "Rules"

[sections.fields.item]
idField = "id"
titleField = "displayName"

[[sections.fields.item.fields]]
type = "number"
key = "rate"
label = "Rate"
min = 0.0
max = 100.0
"#;
        let presentation: SettingsFormV1Presentation =
            toml::from_str(toml_src).expect("valid presentation toml");
        let SettingsFormV1Field::Select { default_value, .. } = &presentation.sections[0].fields[0]
        else {
            unreachable!("expected select field");
        };
        assert_eq!(
            default_value,
            &Some(SelectValue::Str("destination".to_owned()))
        );

        let serialized = toml::to_string_pretty(&presentation).expect("serialize");
        let reparsed: SettingsFormV1Presentation = toml::from_str(&serialized).expect("reparse");
        let SettingsFormV1Field::Select {
            default_value: reparsed_default,
            ..
        } = &reparsed.sections[0].fields[0]
        else {
            unreachable!("expected select field");
        };
        assert_eq!(reparsed_default, default_value);
    }
}
