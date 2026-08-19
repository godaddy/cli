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
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
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
        #[serde(default)]
        description: Option<String>,
        #[serde(default)]
        required: bool,
        #[serde(default)]
        placeholder: Option<String>,
        #[serde(default)]
        min_length: Option<u32>,
        #[serde(default)]
        max_length: Option<u32>,
        #[serde(default)]
        default_value: Option<String>,
    },
    #[serde(rename = "textarea", rename_all = "camelCase")]
    Textarea {
        key: String,
        label: String,
        #[serde(default)]
        description: Option<String>,
        #[serde(default)]
        required: bool,
        #[serde(default)]
        placeholder: Option<String>,
        #[serde(default)]
        min_length: Option<u32>,
        #[serde(default)]
        max_length: Option<u32>,
        #[serde(default)]
        default_value: Option<String>,
    },
    #[serde(rename = "number", rename_all = "camelCase")]
    Number {
        key: String,
        label: String,
        #[serde(default)]
        description: Option<String>,
        #[serde(default)]
        required: bool,
        #[serde(default)]
        min: Option<f64>,
        #[serde(default)]
        max: Option<f64>,
        #[serde(default)]
        step: Option<f64>,
        #[serde(default)]
        suffix: Option<String>,
        #[serde(default)]
        default_value: Option<f64>,
    },
    #[serde(rename = "boolean", rename_all = "camelCase")]
    Boolean {
        key: String,
        label: String,
        #[serde(default)]
        description: Option<String>,
        #[serde(default)]
        required: bool,
        #[serde(default)]
        default_value: Option<bool>,
    },
    #[serde(rename = "select", rename_all = "camelCase")]
    Select {
        key: String,
        label: String,
        #[serde(default)]
        description: Option<String>,
        #[serde(default)]
        required: bool,
        options: Vec<ChoiceOption>,
        #[serde(default)]
        default_value: Option<SelectValue>,
    },
    #[serde(rename = "multi-select", rename_all = "camelCase")]
    MultiSelect {
        key: String,
        label: String,
        #[serde(default)]
        description: Option<String>,
        #[serde(default)]
        required: bool,
        options: Vec<ChoiceOption>,
        #[serde(default)]
        min_items: Option<u32>,
        #[serde(default)]
        max_items: Option<u32>,
        #[serde(default)]
        default_value: Option<Vec<SelectValue>>,
    },
    #[serde(rename = "list-group", rename_all = "camelCase")]
    ListGroup {
        key: String,
        label: String,
        #[serde(default)]
        description: Option<String>,
        #[serde(default)]
        required: bool,
        #[serde(default)]
        min_items: Option<u32>,
        #[serde(default)]
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
    #[serde(default)]
    pub title_field: Option<String>,
    pub fields: Vec<SettingsFormV1Field>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChoiceOption {
    pub value: SelectValue,
    pub label: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SelectValue {
    Str(String),
    Num(f64),
    Bool(bool),
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

/// Structural validation for a `presentation` block: field-name shape,
/// non-empty choice options, and the two uniqueness checks
/// `SettingsFormV1Presentation`'s own Zod `superRefine` runs (unique section
/// keys, unique top-level field keys across sections). Bounds/default
/// consistency and `list-group` depth are left to the API.
pub(super) fn validate_presentation(
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

    #[test]
    fn validate_presentation_accepts_well_formed() {
        let presentation = SettingsFormV1Presentation {
            sections: vec![section("defaults", vec![text_field("calculateUsing")])],
        };
        let mut errors = Vec::new();
        validate_presentation(&presentation, &mut errors, "settings[0].presentation");
        assert!(errors.is_empty(), "{errors:?}");
    }

    #[test]
    fn validate_presentation_rejects_duplicate_section_keys() {
        let presentation = SettingsFormV1Presentation {
            sections: vec![
                section("defaults", vec![text_field("a")]),
                section("defaults", vec![text_field("b")]),
            ],
        };
        let mut errors = Vec::new();
        validate_presentation(&presentation, &mut errors, "settings[0].presentation");
        assert!(
            errors
                .iter()
                .any(|e| e.contains("duplicates another section key")),
            "{errors:?}"
        );
    }

    #[test]
    fn validate_presentation_rejects_duplicate_field_keys_across_sections() {
        let presentation = SettingsFormV1Presentation {
            sections: vec![
                section("a", vec![text_field("shared")]),
                section("b", vec![text_field("shared")]),
            ],
        };
        let mut errors = Vec::new();
        validate_presentation(&presentation, &mut errors, "settings[0].presentation");
        assert!(
            errors
                .iter()
                .any(|e| e.contains("duplicates another field key")),
            "{errors:?}"
        );
    }

    #[test]
    fn validate_presentation_rejects_bad_field_key() {
        let presentation = SettingsFormV1Presentation {
            sections: vec![section("defaults", vec![text_field("bad-key")])],
        };
        let mut errors = Vec::new();
        validate_presentation(&presentation, &mut errors, "settings[0].presentation");
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
        let presentation = SettingsFormV1Presentation {
            sections: vec![section("defaults", vec![field])],
        };
        let mut errors = Vec::new();
        validate_presentation(&presentation, &mut errors, "settings[0].presentation");
        assert!(
            errors.iter().any(|e| e.contains("options must contain")),
            "{errors:?}"
        );
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
