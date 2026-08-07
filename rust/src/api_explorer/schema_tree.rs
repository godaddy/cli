//! Full schema trees and their ids (DEVEX-967's `api schema get <id>`).
//!
//! Unlike `super::schema`'s `schema_type_label`/`summarize_schema`, which
//! stop at one level and collapse a nested object/array to a compact
//! string, [`build_schema_tree`] recurses through every level — `api schema
//! get` never truncates (see `crate::summary::Summary`), so it always
//! returns the complete tree.
//!
//! A schema's id (see [`SchemaLocation`]/[`compute_schema_id`]/
//! [`parse_schema_id`]) is computed at runtime, not minted by
//! `generate-api-catalog`: a `$ref` schema already has a stable name (its
//! existing `$defs` key), and an inline/anonymous schema's dotted path is a
//! pure function of (operationId, where it sits in the operation), so
//! there's nothing to persist ahead of time.

use serde::Serialize;
use serde_json::{Map, Value};

use super::schema::{resolve_schema_ref, schema_base_type_label, schema_type_label};

/// Where a schema sits within an operation — used by `compute_schema_id`
/// when the schema has no `$ref` name of its own, and by `parse_schema_id`
/// to resolve an id back to a concrete schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SchemaLocation {
    RequestBody,
    Parameter(String),
    Response(String),
}

/// Reserved path segments in the dotted-id grammar. An operationId or
/// parameter name that literally equals one of these would break
/// `parse_schema_id`'s left-to-right keyword scan — verified against the
/// real embedded catalog by
/// `schema_id_grammar_has_no_collisions_in_the_real_catalog` below, but not
/// structurally prevented (see the design doc's open questions).
pub(super) const SCHEMA_ID_KEYWORDS: [&str; 3] = ["parameters", "responses", "requestBody"];

/// Computes a stable id for `schema`: its `$defs` key if it's a `$ref`, or a
/// dotted path rooted at `operation_id` if it's inline/anonymous.
pub(super) fn compute_schema_id(
    operation_id: &str,
    location: &SchemaLocation,
    schema: &Value,
) -> String {
    if let Some(name) = schema
        .get("$ref")
        .and_then(Value::as_str)
        .and_then(|r| r.strip_prefix("#/$defs/"))
    {
        return name.to_owned();
    }
    match location {
        SchemaLocation::RequestBody => format!("{operation_id}.requestBody.schema"),
        SchemaLocation::Parameter(name) => format!("{operation_id}.parameters.{name}.schema"),
        SchemaLocation::Response(status) => format!("{operation_id}.responses.{status}.schema"),
    }
}

/// A raw (pre-summarization) schema is a "bare inline scalar" — nothing
/// worth minting a schema id/next_action for — when it's neither a `$ref`
/// (which might resolve to something with real structure) nor an object
/// with declared `properties`. This mirrors `summarize_schema`'s own
/// `"(value)"`-placeholder branch exactly, so the two stay in lockstep:
/// whatever gets flattened to a single placeholder row there gets its type
/// surfaced directly here instead of a drill-down link that would just echo
/// the same type back (e.g. a plain `{"type": "string"}` path parameter).
pub(super) fn scalar_schema_type(
    raw_schema: Option<&Value>,
    defs: &Map<String, Value>,
) -> Option<String> {
    let obj = raw_schema?.as_object()?;
    if obj.contains_key("$ref") || obj.get("properties").and_then(Value::as_object).is_some() {
        return None;
    }
    if !(obj.contains_key("type") || obj.contains_key("enum")) {
        return None;
    }
    Some(schema_type_label(raw_schema?, defs))
}

/// What to show for `raw_schema` in a parameter/response row: the `type`
/// label to display, and whether it's worth minting a `schemaId` alongside
/// it. A bare inline scalar (see `scalar_schema_type`) gets its full type
/// label here since that label *is* the whole story — there's nothing else
/// to drill into. Anything else (a `$ref`, or an inline object with
/// declared `properties`) gets just the coarse base type (e.g. `"object"`
/// for `Shipment`) plus `drillable: true`, since the full detail is a
/// `schema get` away instead of being repeated inline.
pub(super) fn describe_schema(
    raw_schema: Option<&Value>,
    defs: &Map<String, Value>,
) -> (Option<String>, bool) {
    let Some(raw) = raw_schema else {
        return (None, false);
    };
    match scalar_schema_type(Some(raw), defs) {
        Some(full_label) => (Some(full_label), false),
        None => (Some(schema_base_type_label(raw, defs)), true),
    }
}

/// Parses an id produced by `compute_schema_id`'s inline-path branch back
/// into `(operationId, location)`. Scans left-to-right for the first
/// segment matching a reserved keyword rather than assuming a fixed split
/// position, since real operationIds and parameter names can themselves
/// contain literal dots (e.g. `commerce.location.verify-address`,
/// `registeredStores.storeId`).
pub(super) fn parse_schema_id(id: &str) -> Option<(String, SchemaLocation)> {
    let segments: Vec<&str> = id.split('.').collect();
    let keyword_idx = segments
        .iter()
        .position(|segment| SCHEMA_ID_KEYWORDS.contains(segment))?;
    let operation_id = segments[..keyword_idx].join(".");
    if operation_id.is_empty() {
        return None;
    }
    let keyword = segments[keyword_idx];
    let (last, middle) = segments[keyword_idx + 1..].split_last()?;
    if *last != "schema" {
        return None;
    }
    match keyword {
        "requestBody" if middle.is_empty() => Some((operation_id, SchemaLocation::RequestBody)),
        "responses" if middle.len() == 1 => {
            Some((operation_id, SchemaLocation::Response(middle[0].to_owned())))
        }
        "parameters" if !middle.is_empty() => {
            Some((operation_id, SchemaLocation::Parameter(middle.join("."))))
        }
        _ => None,
    }
}

/// One node in a schema's full nested tree — see the module comment above.
#[derive(Debug, Clone, Serialize)]
pub(super) struct SchemaNode {
    pub(super) name: String,
    #[serde(rename = "type")]
    pub(super) node_type: String,
    pub(super) required: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) format: Option<String>,
    #[serde(rename = "enum", skip_serializing_if = "Option::is_none")]
    pub(super) enum_values: Option<Vec<Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) properties: Option<Vec<SchemaNode>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) items: Option<Box<SchemaNode>>,
}

/// Recursively expands `schema` into a `SchemaNode` tree. Reuses
/// `resolve_schema_ref`/`MAX_REF_DEPTH` unchanged for `$ref`-cycle bounding
/// — that bound is orthogonal to how deep object/array nesting itself is
/// allowed to go, which is intentionally unbounded here.
pub(super) fn build_schema_tree(
    name: &str,
    schema: &Value,
    defs: &Map<String, Value>,
) -> SchemaNode {
    let resolved = resolve_schema_ref(schema, defs);
    let Some(obj) = resolved.as_object() else {
        return SchemaNode {
            name: name.to_owned(),
            node_type: "unknown".to_owned(),
            required: false,
            description: None,
            format: None,
            enum_values: None,
            properties: None,
            items: None,
        };
    };
    let description = obj
        .get("description")
        .and_then(Value::as_str)
        .filter(|d| !d.is_empty())
        .map(str::to_owned);
    let format = obj.get("format").and_then(Value::as_str).map(str::to_owned);
    let enum_values = obj.get("enum").and_then(Value::as_array).cloned();

    let Some(type_str) = obj.get("type").and_then(Value::as_str) else {
        // No `type` key: oneOf/anyOf/allOf/enum-only/unresolved-ref — reuse
        // the same fallback vocabulary `schema_type_label` already has for
        // these, rather than duplicating it.
        return SchemaNode {
            name: name.to_owned(),
            node_type: schema_type_label(resolved, defs),
            required: false,
            description,
            format,
            enum_values,
            properties: None,
            items: None,
        };
    };

    if type_str == "object" {
        let required: std::collections::HashSet<&str> = obj
            .get("required")
            .and_then(Value::as_array)
            .map(|arr| arr.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        let properties = obj
            .get("properties")
            .and_then(Value::as_object)
            .map(|props| {
                props
                    .iter()
                    .map(|(prop_name, prop_schema)| {
                        let mut node = build_schema_tree(prop_name, prop_schema, defs);
                        node.required = required.contains(prop_name.as_str());
                        node
                    })
                    .collect()
            });
        return SchemaNode {
            name: name.to_owned(),
            node_type: "object".to_owned(),
            required: false,
            description,
            format,
            enum_values,
            properties,
            items: None,
        };
    }

    if type_str == "array" {
        let items = obj
            .get("items")
            .map(|items_schema| Box::new(build_schema_tree("(item)", items_schema, defs)));
        return SchemaNode {
            name: name.to_owned(),
            node_type: "array".to_owned(),
            required: false,
            description,
            format,
            enum_values,
            properties: None,
            items,
        };
    }

    SchemaNode {
        name: name.to_owned(),
        node_type: type_str.to_owned(),
        required: false,
        description,
        format,
        enum_values,
        properties: None,
        items: None,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        SCHEMA_ID_KEYWORDS, SchemaLocation, build_schema_tree, compute_schema_id, describe_schema,
        parse_schema_id, scalar_schema_type,
    };
    use crate::api_explorer::catalog::catalog;

    fn no_defs() -> serde_json::Map<String, serde_json::Value> {
        serde_json::Map::new()
    }

    #[test]
    fn build_schema_tree_recurses_through_nested_object_levels() {
        let schema = json!({
            "type": "object",
            "required": ["address"],
            "properties": {
                "address": {
                    "type": "object",
                    "required": ["city"],
                    "properties": {
                        "city": { "type": "string" },
                    },
                },
            },
        });
        let tree = build_schema_tree("root", &schema, &no_defs());
        assert_eq!(tree.node_type, "object");
        let address = tree
            .properties
            .as_ref()
            .expect("root has properties")
            .iter()
            .find(|p| p.name == "address")
            .expect("address prop");
        assert!(address.required);
        assert_eq!(address.node_type, "object");
        let city = address
            .properties
            .as_ref()
            .expect("address has properties")
            .iter()
            .find(|p| p.name == "city")
            .expect("city prop");
        assert_eq!(city.node_type, "string");
    }

    #[test]
    fn build_schema_tree_recurses_into_array_items() {
        let schema = json!({
            "type": "array",
            "items": { "type": "object", "properties": { "id": { "type": "string" } } },
        });
        let tree = build_schema_tree("tags", &schema, &no_defs());
        assert_eq!(tree.node_type, "array");
        let item = tree.items.expect("array has an items node");
        assert_eq!(item.node_type, "object");
        assert!(
            item.properties
                .expect("item has properties")
                .iter()
                .any(|p| p.name == "id")
        );
    }

    #[test]
    fn build_schema_tree_ref_cycle_terminates() {
        let mut defs = serde_json::Map::new();
        defs.insert("A".to_owned(), json!({ "$ref": "#/$defs/B" }));
        defs.insert("B".to_owned(), json!({ "$ref": "#/$defs/A" }));
        let schema = json!({ "$ref": "#/$defs/A" });
        let tree = build_schema_tree("root", &schema, &defs);
        assert!(
            tree.node_type.starts_with("ref(#/$defs/"),
            "expected an unresolved ref fallback, got: {}",
            tree.node_type
        );
    }

    #[test]
    fn compute_schema_id_uses_the_defs_key_for_a_ref_schema() {
        let schema = json!({ "$ref": "#/$defs/Business" });
        assert_eq!(
            compute_schema_id("getUser", &SchemaLocation::RequestBody, &schema),
            "Business"
        );
    }

    #[test]
    fn compute_schema_id_builds_a_dotted_path_for_each_inline_location() {
        let schema = json!({ "type": "object" });
        assert_eq!(
            compute_schema_id("getUser", &SchemaLocation::RequestBody, &schema),
            "getUser.requestBody.schema"
        );
        assert_eq!(
            compute_schema_id(
                "getUser",
                &SchemaLocation::Parameter("limit".to_owned()),
                &schema
            ),
            "getUser.parameters.limit.schema"
        );
        assert_eq!(
            compute_schema_id(
                "getUser",
                &SchemaLocation::Response("200".to_owned()),
                &schema
            ),
            "getUser.responses.200.schema"
        );
    }

    #[test]
    fn scalar_schema_type_is_none_for_a_ref_even_if_it_would_resolve_to_a_scalar() {
        let schema = json!({ "$ref": "#/$defs/Confirmation" });
        assert_eq!(scalar_schema_type(Some(&schema), &no_defs()), None);
    }

    #[test]
    fn scalar_schema_type_is_none_for_an_object_with_properties() {
        let schema = json!({ "type": "object", "properties": { "a": {} } });
        assert_eq!(scalar_schema_type(Some(&schema), &no_defs()), None);
    }

    #[test]
    fn scalar_schema_type_is_none_for_a_missing_schema() {
        assert_eq!(scalar_schema_type(None, &no_defs()), None);
    }

    #[test]
    fn scalar_schema_type_labels_a_bare_inline_scalar() {
        let schema = json!({ "type": "string", "format": "uuid" });
        assert_eq!(
            scalar_schema_type(Some(&schema), &no_defs()),
            Some("string(uuid)".to_owned())
        );
    }

    #[test]
    fn scalar_schema_type_labels_a_bare_inline_array() {
        let schema = json!({ "type": "array", "items": { "type": "string" } });
        assert_eq!(
            scalar_schema_type(Some(&schema), &no_defs()),
            Some("array<string>".to_owned())
        );
    }

    #[test]
    fn describe_schema_is_drillable_with_the_base_type_for_a_ref() {
        let mut defs = no_defs();
        defs.insert(
            "Shipment".to_owned(),
            json!({ "type": "object", "properties": { "id": {} } }),
        );
        let schema = json!({ "$ref": "#/$defs/Shipment" });
        assert_eq!(
            describe_schema(Some(&schema), &defs),
            (Some("object".to_owned()), true)
        );
    }

    #[test]
    fn describe_schema_is_drillable_with_the_base_type_for_an_inline_object() {
        let schema = json!({ "type": "object", "properties": { "a": {} } });
        assert_eq!(
            describe_schema(Some(&schema), &no_defs()),
            (Some("object".to_owned()), true)
        );
    }

    #[test]
    fn describe_schema_is_not_drillable_with_the_full_label_for_a_bare_scalar() {
        let schema = json!({ "type": "string", "format": "uuid" });
        assert_eq!(
            describe_schema(Some(&schema), &no_defs()),
            (Some("string(uuid)".to_owned()), false)
        );
    }

    #[test]
    fn describe_schema_is_not_drillable_for_a_missing_schema() {
        assert_eq!(describe_schema(None, &no_defs()), (None, false));
    }

    #[test]
    fn parse_schema_id_round_trips_each_location_kind() {
        assert_eq!(
            parse_schema_id("getUser.requestBody.schema"),
            Some(("getUser".to_owned(), SchemaLocation::RequestBody))
        );
        assert_eq!(
            parse_schema_id("getUser.parameters.limit.schema"),
            Some((
                "getUser".to_owned(),
                SchemaLocation::Parameter("limit".to_owned())
            ))
        );
        assert_eq!(
            parse_schema_id("getUser.responses.200.schema"),
            Some((
                "getUser".to_owned(),
                SchemaLocation::Response("200".to_owned())
            ))
        );
    }

    #[test]
    fn parse_schema_id_handles_dotted_operation_ids_and_parameter_names() {
        // Real catalog data: an operationId and a parameter name can each
        // contain literal dots, so the parser can't assume a fixed split
        // position — it has to scan for the keyword segment.
        assert_eq!(
            parse_schema_id(
                "commerce.location.verify-address.parameters.registeredStores.storeId.schema"
            ),
            Some((
                "commerce.location.verify-address".to_owned(),
                SchemaLocation::Parameter("registeredStores.storeId".to_owned())
            ))
        );
    }

    #[test]
    fn parse_schema_id_rejects_malformed_ids() {
        assert_eq!(parse_schema_id("justAnOperationId"), None);
        assert_eq!(parse_schema_id("getUser.requestBody"), None); // missing trailing "schema"
        assert_eq!(parse_schema_id(".parameters.limit.schema"), None); // empty operationId
    }

    #[test]
    fn schema_id_round_trip_holds_across_the_real_catalog() {
        for domain in catalog() {
            for ep in &domain.endpoints {
                let check = |location: SchemaLocation, schema: &serde_json::Value| {
                    let id = compute_schema_id(&ep.operation_id, &location, schema);
                    if schema.get("$ref").is_some() {
                        // A $ref schema's id is its bare $defs key, resolved
                        // directly against the defs map elsewhere — it's
                        // never round-tripped through parse_schema_id.
                        return;
                    }
                    assert_eq!(
                        parse_schema_id(&id),
                        Some((ep.operation_id.clone(), location)),
                        "id was {id:?}"
                    );
                };

                if let Some(schema) = ep.request_body.as_ref().and_then(|b| b.get("schema")) {
                    check(SchemaLocation::RequestBody, schema);
                }
                for param in &ep.parameters {
                    if let (Some(name), Some(schema)) = (
                        param.get("name").and_then(serde_json::Value::as_str),
                        param.get("schema"),
                    ) {
                        check(SchemaLocation::Parameter(name.to_owned()), schema);
                    }
                }
                if let Some(responses) = ep.responses.as_object() {
                    for (status, resp) in responses {
                        if let Some(schema) = resp.get("schema") {
                            check(SchemaLocation::Response(status.clone()), schema);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn schema_id_grammar_has_no_collisions_in_the_real_catalog() {
        let collides = |segment: &str| SCHEMA_ID_KEYWORDS.contains(&segment) || segment == "schema";
        for domain in catalog() {
            for ep in &domain.endpoints {
                for segment in ep.operation_id.split('.') {
                    assert!(
                        !collides(segment),
                        "operationId {:?} has a segment ({segment:?}) that collides with the \
                         schema-id grammar",
                        ep.operation_id
                    );
                }
                for param in &ep.parameters {
                    let Some(name) = param.get("name").and_then(serde_json::Value::as_str) else {
                        continue;
                    };
                    for segment in name.split('.') {
                        assert!(
                            !collides(segment),
                            "parameter {name:?} on {:?} has a segment ({segment:?}) that \
                             collides with the schema-id grammar",
                            ep.operation_id
                        );
                    }
                }
            }
        }
    }
}
