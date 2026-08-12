//! `api parameter`/`api response` — first-class entities scoped by
//! `--operation`, backing `api parameter list/get` and `api response
//! list/get`. `api operation get` summarizes the same rows for its own
//! preview, and also uses [`summarize_graphql_schema`] for its GraphQL
//! summary.

use serde::Serialize;
use serde_json::{Map, Value, json};

use cli_engine::CliCoreError;

use crate::summary::Summary;

use super::catalog::{Endpoint, GraphqlSchema};
use super::schema::{SchemaSummaryProperty, summarize_schema};
use super::schema_tree::{SchemaLocation, compute_schema_id, describe_schema};

/// Breadth cap on a parameter's/response's *embedded* schema preview — the
/// full, untruncated tree is always one `api schema get <id>` away (see
/// `super::schema::build_schema_tree`), so this only bounds how much of it
/// gets inlined here.
const SCHEMA_PROPERTY_PREVIEW_CAP: usize = 20;

/// Default page size for `api parameter list`/`api response list` (via
/// `CommandSpec::with_pagination`) when neither `--limit` nor `--offset` is
/// passed, and for `api operation get`'s own embedded parameter/response
/// previews (which aren't paginated themselves — see `Summary::capped`
/// there — but use the same "how many rows to show inline" size).
pub(super) const OPERATION_CHILD_LIST_DEFAULT_LIMIT: usize = 20;

/// Upper bound a user can request with `--limit` on `api parameter list`/
/// `api response list`. Generous relative to the real catalog (26
/// parameters, 11 responses is the current max of either), just to guard
/// against a client asking for something absurd.
pub(super) const OPERATION_CHILD_LIST_MAX_LIMIT: i64 = 200;

#[derive(Debug, Clone, Serialize)]
pub(super) struct ParameterSummary {
    pub(super) name: String,

    #[serde(rename = "in")]
    pub(super) location: String,

    pub(super) required: bool,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) description: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) schema: Option<Summary<SchemaSummaryProperty>>,

    /// Id for `api schema get`, present whenever this parameter's schema has
    /// something to drill into — an object with properties, or a `$ref`
    /// (which might resolve to one) — regardless of whether the embedded
    /// preview above happens to be truncated, so a caller always knows what
    /// shape a parameter (most importantly `body`) expects. `None` for a
    /// bare inline scalar (see `scalar_type` below), where drilling in would
    /// just echo the same type back.
    #[serde(rename = "schemaId", skip_serializing_if = "Option::is_none")]
    pub(super) schema_id: Option<String>,

    /// The type to show alongside `schema_id`/`schema`: the full label
    /// (e.g. `"string(uuid)"`, `"array<string>"`) when `schema_id` is
    /// absent (a bare scalar — the label is the whole story), or just the
    /// coarse base type (e.g. `"object"`) when `schema_id` *is* present,
    /// since the full nested detail is a `schema get` away instead of being
    /// repeated here. Always present whenever there's a schema at all — see
    /// `describe_schema`.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub(super) scalar_type: Option<String>,
}

/// Every parameter on `ep`, plus — if it has a request body — a synthetic
/// `{name: "body", in: "body"}` row. In the catalog's flattened shape
/// (`ep.request_body`: `required`/`contentType`/`schema`, no `name`/`in` of
/// its own), a request body is structurally just a parameter missing a
/// name, so this is the one place both are normalized into the same shape.
pub(super) fn summarize_parameters(
    ep: &Endpoint,
    defs: &Map<String, Value>,
) -> Vec<ParameterSummary> {
    let mut rows: Vec<ParameterSummary> = ep
        .parameters
        .iter()
        .filter_map(|param| {
            let obj = param.as_object()?;
            let name = obj.get("name").and_then(Value::as_str)?.to_owned();
            let location = obj
                .get("in")
                .and_then(Value::as_str)
                .unwrap_or("query")
                .to_owned();
            let required = obj
                .get("required")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let description = obj
                .get("description")
                .and_then(Value::as_str)
                .map(str::to_owned);
            let raw_schema = obj.get("schema");
            let schema = summarize_schema(raw_schema, defs)
                .map(|props| Summary::capped(props, SCHEMA_PROPERTY_PREVIEW_CAP));
            let (scalar_type, drillable) = describe_schema(raw_schema, defs);
            let schema_id = if drillable {
                raw_schema.map(|raw| {
                    compute_schema_id(
                        &ep.operation_id,
                        &SchemaLocation::Parameter(name.clone()),
                        raw,
                    )
                })
            } else {
                None
            };
            Some(ParameterSummary {
                name,
                location,
                required,
                description,
                schema,
                schema_id,
                scalar_type,
            })
        })
        .collect();

    if let Some(body) = &ep.request_body {
        let required = body
            .get("required")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let description = body
            .get("description")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let raw_schema = body.get("schema");
        let schema = summarize_schema(raw_schema, defs)
            .map(|props| Summary::capped(props, SCHEMA_PROPERTY_PREVIEW_CAP));
        let (scalar_type, drillable) = describe_schema(raw_schema, defs);
        let schema_id = if drillable {
            raw_schema
                .map(|raw| compute_schema_id(&ep.operation_id, &SchemaLocation::RequestBody, raw))
        } else {
            None
        };
        rows.push(ParameterSummary {
            name: "body".to_owned(),
            location: "body".to_owned(),
            required,
            description,
            schema,
            schema_id,
            scalar_type,
        });
    }

    rows
}

/// Finds the raw (pre-summarization) schema `Value` for one of `ep`'s
/// parameters by name, and the `SchemaLocation` to compute its id with.
/// Handles the synthetic `"body"` name specially: unlike every other
/// parameter, `body` isn't in `ep.parameters` at all — it's `ep.request_body`
/// wearing a parameter-shaped hat (see `summarize_parameters` above), so its
/// schema id must use the `RequestBody` location, not `Parameter("body")`.
pub(super) fn find_parameter_schema<'a>(
    ep: &'a Endpoint,
    name: &str,
) -> Option<(&'a Value, SchemaLocation)> {
    if name == "body" {
        return ep
            .request_body
            .as_ref()
            .and_then(|b| b.get("schema"))
            .map(|schema| (schema, SchemaLocation::RequestBody));
    }
    ep.parameters.iter().find_map(|param| {
        let is_match = param.get("name").and_then(Value::as_str) == Some(name);
        if !is_match {
            return None;
        }
        param
            .get("schema")
            .map(|schema| (schema, SchemaLocation::Parameter(name.to_owned())))
    })
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct ResponseRow {
    pub(super) status: String,
    pub(super) description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) schema: Option<Summary<SchemaSummaryProperty>>,
    /// See `ParameterSummary::schema_id` — same "nothing to drill into"
    /// exception for a bare inline scalar.
    #[serde(rename = "schemaId", skip_serializing_if = "Option::is_none")]
    pub(super) schema_id: Option<String>,
    /// See `ParameterSummary::scalar_type`.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub(super) scalar_type: Option<String>,
}

/// Every response on `ep`, sorted by status for a stable listing order
/// (`ep.responses` is parsed straight from JSON and carries no ordering
/// guarantee of its own).
pub(super) fn response_rows(ep: &Endpoint, defs: &Map<String, Value>) -> Vec<ResponseRow> {
    let Some(responses) = ep.responses.as_object() else {
        return Vec::new();
    };
    let mut rows: Vec<ResponseRow> = responses
        .iter()
        .map(|(status, resp)| {
            let description = resp
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            let raw_schema = resp.get("schema");
            let schema = summarize_schema(raw_schema, defs)
                .map(|props| Summary::capped(props, SCHEMA_PROPERTY_PREVIEW_CAP));
            let (scalar_type, drillable) = describe_schema(raw_schema, defs);
            let schema_id = if drillable {
                raw_schema.map(|raw| {
                    compute_schema_id(
                        &ep.operation_id,
                        &SchemaLocation::Response(status.clone()),
                        raw,
                    )
                })
            } else {
                None
            };
            ResponseRow {
                status: status.clone(),
                description,
                scalar_type,
                schema,
                schema_id,
            }
        })
        .collect();
    rows.sort_by(|a, b| a.status.cmp(&b.status));
    rows
}

/// Serializes `value` to a JSON `Value`, mapping the (practically
/// unreachable for these plain-data types) serialization failure to a
/// proper `CliCoreError` instead of panicking — mirrors the pattern already
/// used in `webhook::group`.
pub(super) fn to_json(value: impl Serialize) -> Result<Value, CliCoreError> {
    serde_json::to_value(value).map_err(|e| {
        crate::error::GddyError::unexpected(format!("failed to serialize output: {e}"))
            .into_cli_error()
    })
}

// ---------------------------------------------------------------------------
// GraphQL summarization — condenses a domain's embedded GraphQL schema
// (parsed at catalog-build time) into a per-operation summary for
// `api operation get`. Ported from the original TypeScript CLI's
// `summarizeGraphqlSchema`, minus its operation-count cap: every operation
// is included, full stop — no truncation until there's a real mechanism
// for retrieving what got cut.
// ---------------------------------------------------------------------------

pub(super) fn summarize_graphql_schema(graphql: &GraphqlSchema) -> Value {
    let query_count = graphql
        .operations
        .iter()
        .filter(|op| op.kind == "query")
        .count();
    let mutation_count = graphql.operations.len() - query_count;
    let operations: Vec<Value> = graphql
        .operations
        .iter()
        .map(|op| {
            json!({
                "name": op.name,
                "kind": op.kind,
                "returnType": op.return_type,
                "description": op.description,
                "deprecated": op.deprecated,
                "deprecationReason": op.deprecation_reason,
                "args": op.args.iter().map(|a| json!({
                    "name": a.name,
                    "type": a.arg_type,
                    "required": a.required,
                    "description": a.description,
                    "defaultValue": a.default_value,
                })).collect::<Vec<_>>(),
            })
        })
        .collect();
    let types: Vec<Value> = graphql
        .types
        .iter()
        .map(|t| {
            json!({
                "name": t.name,
                "kind": t.kind,
                "fields": t.fields.iter().map(|f| json!({
                    "name": f.name,
                    "type": f.field_type,
                    "description": f.description,
                })).collect::<Vec<_>>(),
                "values": t.values,
            })
        })
        .collect();
    json!({
        "schemaRef": graphql.schema_ref,
        "operationCount": graphql.operation_count,
        "queryCount": query_count,
        "mutationCount": mutation_count,
        "operations": operations,
        "typeCount": types.len(),
        "types": types,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::catalog::{GraphqlArgument, GraphqlOperation, GraphqlSchema};
    use super::summarize_graphql_schema;

    fn make_graphql_operations(count: usize) -> Vec<GraphqlOperation> {
        (0..count)
            .map(|i| GraphqlOperation {
                name: format!("op{i}"),
                kind: if i % 2 == 0 { "query" } else { "mutation" }.to_owned(),
                return_type: "String".to_owned(),
                description: None,
                deprecated: false,
                deprecation_reason: None,
                args: vec![],
            })
            .collect()
    }

    #[test]
    fn summarize_graphql_schema_includes_every_operation_and_splits_query_mutation_counts() {
        let schema = GraphqlSchema {
            schema_ref: "./schema.graphql".to_owned(),
            operation_count: 25,
            operations: make_graphql_operations(25),
            types: vec![],
            sdl: String::new(),
        };
        let summary = summarize_graphql_schema(&schema);
        assert_eq!(summary["operationCount"], json!(25));
        assert_eq!(summary["queryCount"], json!(13));
        assert_eq!(summary["mutationCount"], json!(12));
        assert_eq!(
            summary["operations"]
                .as_array()
                .expect("operations array")
                .len(),
            25
        );
    }

    #[test]
    fn summarize_graphql_schema_carries_operation_detail() {
        let schema = GraphqlSchema {
            schema_ref: "./schema.graphql".to_owned(),
            operation_count: 1,
            operations: vec![GraphqlOperation {
                name: "widgets".to_owned(),
                kind: "query".to_owned(),
                return_type: "[Widget]".to_owned(),
                description: Some("Lists widgets.".to_owned()),
                deprecated: true,
                deprecation_reason: Some("use widgetsV2".to_owned()),
                args: vec![GraphqlArgument {
                    name: "id".to_owned(),
                    arg_type: "String!".to_owned(),
                    required: true,
                    description: Some("The widget id.".to_owned()),
                    default_value: None,
                }],
            }],
            types: vec![],
            sdl: String::new(),
        };
        let summary = summarize_graphql_schema(&schema);
        assert_eq!(summary["operations"][0]["name"], json!("widgets"));
        assert_eq!(summary["operations"][0]["deprecated"], json!(true));
        assert_eq!(
            summary["operations"][0]["description"],
            json!("Lists widgets.")
        );
        assert_eq!(
            summary["operations"][0]["args"][0]["type"],
            json!("String!")
        );
        assert_eq!(
            summary["operations"][0]["args"][0]["description"],
            json!("The widget id.")
        );
        assert_eq!(summary["typeCount"], json!(0));
    }

    #[test]
    fn summarize_graphql_schema_carries_type_detail() {
        use super::super::catalog::{GraphqlField, GraphqlType};

        let schema = GraphqlSchema {
            schema_ref: "./schema.graphql".to_owned(),
            operation_count: 0,
            operations: vec![],
            types: vec![
                GraphqlType {
                    name: "Widget".to_owned(),
                    kind: "object".to_owned(),
                    fields: vec![GraphqlField {
                        name: "id".to_owned(),
                        field_type: "ID!".to_owned(),
                        description: Some("The widget id.".to_owned()),
                    }],
                    values: vec![],
                },
                GraphqlType {
                    name: "WidgetStatus".to_owned(),
                    kind: "enum".to_owned(),
                    fields: vec![],
                    values: vec!["ACTIVE".to_owned(), "RETIRED".to_owned()],
                },
            ],
            sdl: "type Widget { id: ID! }".to_owned(),
        };
        let summary = summarize_graphql_schema(&schema);
        assert_eq!(summary["typeCount"], json!(2));
        assert_eq!(summary["types"][0]["name"], json!("Widget"));
        assert_eq!(summary["types"][0]["fields"][0]["name"], json!("id"));
        assert_eq!(summary["types"][1]["values"], json!(["ACTIVE", "RETIRED"]));
    }
}
