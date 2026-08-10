//! One-level schema formatting: resolving `$ref`s against a domain's
//! `$defs`, and condensing a schema fragment down to either a short type
//! label or a one-level property summary.
//!
//! [`schema_type_label`]/[`summarize_schema`] stop at one level and
//! collapse a nested object/array to a compact string — used by `api
//! operation get` and the `api parameter`/`api response` previews (see
//! `crate::api_explorer::summary`). The complete, never-truncated nested
//! tree behind `api schema get` lives in [`super::schema_tree`], which
//! builds on [`resolve_schema_ref`] and [`schema_type_label`] here.

use serde::Serialize;
use serde_json::{Map, Value};

// ---------------------------------------------------------------------------
// Schema summarization — condenses a raw JSON-schema fragment down to
// top-level property names/types for `api operation get`, so a caller sees a
// scannable summary instead of a full nested schema dump. Ported from the
// original TypeScript CLI's `summarizeSchema`/`schemaTypeLabel` family.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub(super) struct SchemaSummaryProperty {
    pub(super) name: String,
    #[serde(rename = "type")]
    pub(super) prop_type: String,
    pub(super) required: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) format: Option<String>,
    #[serde(rename = "enum", skip_serializing_if = "Option::is_none")]
    pub(super) enum_values: Option<Vec<Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) items: Option<String>,
}

/// Max `$ref` hops to follow when resolving a schema fragment against a
/// domain's `$defs` — bounds a def-to-def cycle (e.g. A refers to B refers
/// back to A) rather than looping forever.
pub(super) const MAX_REF_DEPTH: u8 = 5;

/// Follows a `{"$ref": "#/$defs/Name"}` pointer against `defs`, transparently
/// substituting the referenced schema, up to `MAX_REF_DEPTH` hops (a def can
/// itself be a `$ref` to another def). Returns `schema` unchanged if it
/// isn't a `$ref`, or the last-resolved schema if the chain doesn't resolve
/// (unknown def name, external non-`#/$defs/` ref, or depth exceeded) —
/// callers fall back to rendering that as an unresolved `ref(...)`.
///
/// `pub(super)` (rather than private) because `super::schema_tree`'s
/// `build_schema_tree` needs the exact same `$ref`-cycle bound.
pub(super) fn resolve_schema_ref<'a>(schema: &'a Value, defs: &'a Map<String, Value>) -> &'a Value {
    let mut current = schema;
    for _ in 0..MAX_REF_DEPTH {
        let Some(key) = current
            .get("$ref")
            .and_then(Value::as_str)
            .and_then(|r| r.strip_prefix("#/$defs/"))
        else {
            break;
        };
        let Some(resolved) = defs.get(key) else {
            break;
        };
        current = resolved;
    }
    current
}

/// A short human-readable label for a JSON-schema fragment, e.g.
/// `array<object{id, name, ...}>`, `enum(a|b|c)`, `string(uuid)`. Resolves
/// `$ref`s against `defs` first, so a referenced schema is described the
/// same as if it were inlined.
pub(super) fn schema_type_label(schema: &Value, defs: &Map<String, Value>) -> String {
    let schema = resolve_schema_ref(schema, defs);
    let Some(obj) = schema.as_object() else {
        return "unknown".to_owned();
    };
    if let Some(type_str) = obj.get("type").and_then(Value::as_str) {
        if type_str == "array"
            && let Some(items) = obj.get("items")
        {
            return format!("array<{}>", schema_type_label(items, defs));
        }
        if type_str == "object"
            && let Some(props) = obj.get("properties").and_then(Value::as_object)
        {
            let names: Vec<&str> = props.keys().map(String::as_str).collect();
            return format!("object{{{}}}", names.join(", "));
        }
        return match obj.get("format").and_then(Value::as_str) {
            Some(format) => format!("{type_str}({format})"),
            None => type_str.to_owned(),
        };
    }
    if let Some(enum_vals) = obj.get("enum").and_then(Value::as_array) {
        let vals: Vec<String> = enum_vals.iter().map(json_value_to_label).collect();
        return format!("enum({})", vals.join("|"));
    }
    if obj.get("oneOf").and_then(Value::as_array).is_some() {
        return "oneOf".to_owned();
    }
    if obj.get("anyOf").and_then(Value::as_array).is_some() {
        return "anyOf".to_owned();
    }
    if obj.get("allOf").and_then(Value::as_array).is_some() {
        return "allOf".to_owned();
    }
    // Only reached for a $ref that resolve_schema_ref couldn't follow
    // (unknown def, external ref, or a chain deeper than MAX_REF_DEPTH).
    if let Some(reference) = obj.get("$ref").and_then(Value::as_str) {
        return format!("ref({reference})");
    }
    "object".to_owned()
}

/// The same fallback chain as `schema_type_label`, but never recurses into
/// array items or lists an object's property names — just the bare kind
/// (`"object"`, `"array"`, `"string"`, ...). Used for a schema that already
/// has a `schemaId` pointing at its full nested detail via `api schema
/// get`: repeating that detail here too would be redundant, and for an
/// object with many properties, unbounded.
pub(super) fn schema_base_type_label(schema: &Value, defs: &Map<String, Value>) -> String {
    let schema = resolve_schema_ref(schema, defs);
    let Some(obj) = schema.as_object() else {
        return "unknown".to_owned();
    };
    if let Some(type_str) = obj.get("type").and_then(Value::as_str) {
        return type_str.to_owned();
    }
    if obj.get("enum").and_then(Value::as_array).is_some() {
        return "enum".to_owned();
    }
    if obj.get("oneOf").and_then(Value::as_array).is_some() {
        return "oneOf".to_owned();
    }
    if obj.get("anyOf").and_then(Value::as_array).is_some() {
        return "anyOf".to_owned();
    }
    if obj.get("allOf").and_then(Value::as_array).is_some() {
        return "allOf".to_owned();
    }
    if let Some(reference) = obj.get("$ref").and_then(Value::as_str) {
        return format!("ref({reference})");
    }
    "object".to_owned()
}

/// Mimics JS `Array.prototype.join`'s implicit `String(value)` coercion for
/// the handful of JSON value kinds an enum entry can be.
fn json_value_to_label(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        other => other.to_string(),
    }
}

pub(super) fn summarize_schema(
    schema: Option<&Value>,
    defs: &Map<String, Value>,
) -> Option<Vec<SchemaSummaryProperty>> {
    let schema = resolve_schema_ref(schema?, defs);
    let obj = schema.as_object()?;
    let Some(properties) = obj.get("properties").and_then(Value::as_object) else {
        if obj.contains_key("type") || obj.contains_key("enum") {
            return Some(vec![SchemaSummaryProperty {
                name: "(value)".to_owned(),
                prop_type: schema_type_label(schema, defs),
                required: true,
                description: None,
                format: None,
                enum_values: None,
                items: None,
            }]);
        }
        return None;
    };
    let required: std::collections::HashSet<&str> = obj
        .get("required")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    Some(
        properties
            .iter()
            .map(|(name, prop)| {
                let prop_obj = prop.as_object();
                // A property that's itself a bare `$ref` carries none of
                // description/format/enum/items locally — fall back to the
                // resolved def's, while still preferring a local value if
                // the property overrides it (JSON Schema allows keywords
                // alongside `$ref`).
                let resolved_obj = resolve_schema_ref(prop, defs).as_object();
                let description = prop_obj
                    .and_then(|p| p.get("description"))
                    .or_else(|| resolved_obj.and_then(|p| p.get("description")))
                    .and_then(Value::as_str)
                    .filter(|d| !d.is_empty())
                    .map(str::to_owned);
                let format = prop_obj
                    .and_then(|p| p.get("format"))
                    .or_else(|| resolved_obj.and_then(|p| p.get("format")))
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                let enum_values = prop_obj
                    .and_then(|p| p.get("enum"))
                    .or_else(|| resolved_obj.and_then(|p| p.get("enum")))
                    .and_then(Value::as_array)
                    .cloned();
                let items = resolved_obj
                    .filter(|p| p.get("type").and_then(Value::as_str) == Some("array"))
                    .and_then(|p| p.get("items"))
                    .filter(|v| v.is_object())
                    .map(|items| schema_type_label(items, defs));
                SchemaSummaryProperty {
                    name: name.clone(),
                    prop_type: schema_type_label(prop, defs),
                    required: required.contains(name.as_str()),
                    description,
                    format,
                    enum_values,
                    items,
                }
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{schema_base_type_label, schema_type_label, summarize_schema};

    fn no_defs() -> serde_json::Map<String, serde_json::Value> {
        serde_json::Map::new()
    }

    #[test]
    fn schema_type_label_object_lists_every_property_name() {
        let schema = json!({
            "type": "object",
            "properties": { "a": {}, "b": {}, "c": {}, "d": {}, "e": {}, "f": {} },
        });
        assert_eq!(
            schema_type_label(&schema, &no_defs()),
            "object{a, b, c, d, e, f}"
        );
    }

    #[test]
    fn schema_type_label_array_recurses_into_items() {
        let schema = json!({ "type": "array", "items": { "type": "string" } });
        assert_eq!(schema_type_label(&schema, &no_defs()), "array<string>");
    }

    #[test]
    fn schema_type_label_bare_array_of_untyped_objects() {
        // No declared `properties` on the item schema — renders as the bare
        // "array<object>" (no trailing `{`), distinct from "array<object{...}>"
        // for an item schema with declared properties.
        let schema =
            json!({ "type": "array", "items": { "type": "object", "additionalProperties": true } });
        assert_eq!(schema_type_label(&schema, &no_defs()), "array<object>");
    }

    #[test]
    fn schema_type_label_string_with_format() {
        let schema = json!({ "type": "string", "format": "uuid" });
        assert_eq!(schema_type_label(&schema, &no_defs()), "string(uuid)");
    }

    #[test]
    fn schema_type_label_enum_lists_every_value() {
        let schema = json!({ "enum": ["a", "b", "c", "d", "e", "f", "g", "h", "i"] });
        assert_eq!(
            schema_type_label(&schema, &no_defs()),
            "enum(a|b|c|d|e|f|g|h|i)"
        );
    }

    #[test]
    fn schema_type_label_falls_back_through_one_of_any_of_all_of_and_ref() {
        assert_eq!(
            schema_type_label(&json!({ "oneOf": [] }), &no_defs()),
            "oneOf"
        );
        assert_eq!(
            schema_type_label(&json!({ "anyOf": [] }), &no_defs()),
            "anyOf"
        );
        assert_eq!(
            schema_type_label(&json!({ "allOf": [] }), &no_defs()),
            "allOf"
        );
        assert_eq!(
            schema_type_label(&json!({ "$ref": "#/$defs/uuid" }), &no_defs()),
            "ref(#/$defs/uuid)"
        );
        assert_eq!(schema_type_label(&json!({}), &no_defs()), "object");
    }

    #[test]
    fn schema_type_label_resolves_a_ref_against_defs() {
        let mut defs = serde_json::Map::new();
        defs.insert(
            "Business".to_owned(),
            json!({ "type": "object", "properties": { "name": {}, "address": {} } }),
        );
        let schema = json!({ "$ref": "#/$defs/Business" });
        assert_eq!(schema_type_label(&schema, &defs), "object{address, name}");
    }

    #[test]
    fn schema_type_label_ref_chain_through_multiple_defs() {
        let mut defs = serde_json::Map::new();
        defs.insert("A".to_owned(), json!({ "$ref": "#/$defs/B" }));
        defs.insert(
            "B".to_owned(),
            json!({ "type": "string", "format": "uuid" }),
        );
        let schema = json!({ "$ref": "#/$defs/A" });
        assert_eq!(schema_type_label(&schema, &defs), "string(uuid)");
    }

    #[test]
    fn schema_type_label_ref_cycle_does_not_hang() {
        let mut defs = serde_json::Map::new();
        defs.insert("A".to_owned(), json!({ "$ref": "#/$defs/B" }));
        defs.insert("B".to_owned(), json!({ "$ref": "#/$defs/A" }));
        let schema = json!({ "$ref": "#/$defs/A" });
        // Bounded by MAX_REF_DEPTH — must terminate and fall back to an
        // unresolved ref label rather than looping forever. Which of A/B it
        // lands on depends on MAX_REF_DEPTH's parity, so only assert the
        // shape, not the exact key.
        assert!(
            schema_type_label(&schema, &defs).starts_with("ref(#/$defs/"),
            "expected an unresolved ref fallback, got: {}",
            schema_type_label(&schema, &defs)
        );
    }

    #[test]
    fn schema_type_label_unknown_ref_falls_back_to_unresolved_label() {
        let schema = json!({ "$ref": "#/$defs/DoesNotExist" });
        assert_eq!(
            schema_type_label(&schema, &no_defs()),
            "ref(#/$defs/DoesNotExist)"
        );
    }

    #[test]
    fn summarize_schema_keeps_the_full_description() {
        let long_description = "x".repeat(200);
        let schema = json!({
            "type": "object",
            "properties": { "note": { "type": "string", "description": long_description.clone() } },
        });
        let props = summarize_schema(Some(&schema), &no_defs()).expect("has properties");
        let note = props.iter().find(|p| p.name == "note").expect("note prop");
        assert_eq!(note.description.as_deref(), Some(long_description.as_str()));
    }

    #[test]
    fn summarize_schema_keeps_the_full_enum_regardless_of_size() {
        let big_enum: Vec<_> = (0..20).map(|i| json!(i)).collect();
        let schema = json!({
            "type": "object",
            "properties": { "big": { "type": "integer", "enum": big_enum.clone() } },
        });
        let props = summarize_schema(Some(&schema), &no_defs()).expect("has properties");
        let big = props.iter().find(|p| p.name == "big").expect("big prop");
        assert_eq!(big.enum_values.as_ref(), Some(&big_enum));
    }

    #[test]
    fn summarize_schema_scalar_value_falls_back_to_value_placeholder() {
        let schema = json!({ "type": "string" });
        let props = summarize_schema(Some(&schema), &no_defs()).expect("scalar schema summarizes");
        assert_eq!(props.len(), 1);
        assert_eq!(props[0].name, "(value)");
        assert_eq!(props[0].prop_type, "string");
        assert!(props[0].required);
    }

    #[test]
    fn summarize_schema_none_for_schema_with_no_type_or_properties() {
        assert!(summarize_schema(Some(&json!({})), &no_defs()).is_none());
        assert!(summarize_schema(None, &no_defs()).is_none());
    }

    #[test]
    fn summarize_schema_resolves_a_top_level_ref() {
        let mut defs = serde_json::Map::new();
        defs.insert(
            "Business".to_owned(),
            json!({
                "type": "object",
                "properties": { "name": { "type": "string" } },
            }),
        );
        // Most catalog request bodies are exactly this shape: a bare `$ref`
        // with no inline `properties` for summarize_schema to find directly.
        let schema = json!({ "$ref": "#/$defs/Business" });
        let props = summarize_schema(Some(&schema), &defs).expect("resolves through $ref");
        assert_eq!(props.len(), 1);
        assert_eq!(props[0].name, "name");
        assert_eq!(props[0].prop_type, "string");
    }

    #[test]
    fn summarize_schema_resolves_a_property_level_ref() {
        let mut defs = serde_json::Map::new();
        defs.insert(
            "address".to_owned(),
            json!({
                "type": "object",
                "properties": { "street": {}, "city": {} },
                "description": "A mailing address",
            }),
        );
        let schema = json!({
            "type": "object",
            "properties": { "address": { "$ref": "#/$defs/address" } },
        });
        let props = summarize_schema(Some(&schema), &defs).expect("has properties");
        let address = props
            .iter()
            .find(|p| p.name == "address")
            .expect("address prop");
        assert_eq!(address.prop_type, "object{city, street}");
        // No local description at the reference site — falls back to the
        // resolved def's.
        assert_eq!(address.description.as_deref(), Some("A mailing address"));
    }

    #[test]
    fn summarize_schema_local_description_overrides_the_resolved_def() {
        let mut defs = serde_json::Map::new();
        defs.insert(
            "address".to_owned(),
            json!({ "type": "object", "properties": {}, "description": "def description" }),
        );
        let schema = json!({
            "type": "object",
            "properties": {
                "address": { "$ref": "#/$defs/address", "description": "local override" },
            },
        });
        let props = summarize_schema(Some(&schema), &defs).expect("has properties");
        let address = props
            .iter()
            .find(|p| p.name == "address")
            .expect("address prop");
        assert_eq!(address.description.as_deref(), Some("local override"));
    }

    #[test]
    fn schema_base_type_label_does_not_list_property_names_or_recurse_into_items() {
        let object_schema = json!({
            "type": "object",
            "properties": { "a": {}, "b": {}, "c": {} },
        });
        assert_eq!(schema_base_type_label(&object_schema, &no_defs()), "object");

        let array_schema =
            json!({ "type": "array", "items": { "type": "object", "properties": { "a": {} } } });
        assert_eq!(schema_base_type_label(&array_schema, &no_defs()), "array");
    }

    #[test]
    fn schema_base_type_label_resolves_a_ref_to_its_base_type() {
        let mut defs = no_defs();
        defs.insert(
            "Shipment".to_owned(),
            json!({ "type": "object", "properties": { "id": {} } }),
        );
        let schema = json!({ "$ref": "#/$defs/Shipment" });
        assert_eq!(schema_base_type_label(&schema, &defs), "object");
    }
}
