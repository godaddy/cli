use std::sync::OnceLock;

use cli_engine::{
    CliCoreError, CommandResult, CommandSpec, GroupSpec, Module, NextActionParam, PaginationConfig,
    RuntimeCommandSpec, RuntimeGroupSpec, TableColumn, Tier,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::next_action::{next_action, required_value};
use crate::output_schema::output_schema;
use crate::summary::Summary;

output_schema!(ApiDomain {
    "domain": "string";
    "title": "string";
    "description": "string", optional;
    "endpoints": "number";
    "baseUrl": "string";
});

output_schema!(ApiEndpoint {
    "domain": "string";
    "operationId": "string";
    "method": "string";
    "path": "string";
    "summary": "string", optional;
    "scopes": "[]string";
    "graphqlOperations": "number", optional;
});

// `api operation list --domain X` lists endpoints within one domain, so each row
// omits the (redundant) domain field that the cross-domain `api search` emits.
output_schema!(ApiDomainEndpoint {
    "operationId": "string";
    "method": "string";
    "path": "string";
    "summary": "string", optional;
    "scopes": "[]string";
    "graphqlOperations": "number", optional;
});

output_schema!(ApiOperation {
    "domain": "string";
    "baseUrl": "string";
    "operationId": "string";
    "method": "string";
    "path": "string";
    "fullPath": "string";
    "summary": "string", optional;
    "description": "string", optional;
    "parameters": "object";
    "responses": "object";
    "scopes": "[]string";
    "graphql": "object", optional;
    "message": "string", optional;
    "matches": "[]object", optional;
});

// ---------------------------------------------------------------------------
// Catalog types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct Domain {
    name: String,
    title: String,
    description: String,
    #[serde(rename = "baseUrl")]
    base_url: String,
    endpoints: Vec<Endpoint>,
    /// Local JSON-schema definitions (`$defs`) that a `requestBody`/response
    /// schema's `$ref` may point at, e.g. `{"$ref": "#/$defs/Business"}`.
    /// Most catalog request bodies are a bare `$ref` with no inline
    /// `properties`, so resolving these is required for schema
    /// summarization to say anything useful about them.
    #[serde(rename = "$defs", default)]
    defs: Map<String, Value>,
}

#[derive(Debug, Deserialize)]
struct Endpoint {
    #[serde(rename = "operationId")]
    operation_id: String,
    method: String,
    path: String,
    summary: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    parameters: Vec<Value>,
    #[serde(rename = "requestBody", default)]
    request_body: Option<Value>,
    #[serde(default)]
    responses: Value,
    #[serde(default)]
    scopes: Vec<String>,
    #[serde(default)]
    graphql: Option<GraphqlSchema>,
}

#[derive(Debug, Deserialize)]
struct GraphqlArgument {
    name: String,
    #[serde(rename = "type")]
    arg_type: String,
    required: bool,
    #[serde(default, rename = "defaultValue")]
    default_value: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GraphqlOperation {
    name: String,
    kind: String,
    #[serde(rename = "returnType")]
    return_type: String,
    #[serde(default)]
    deprecated: bool,
    #[serde(default, rename = "deprecationReason")]
    deprecation_reason: Option<String>,
    #[serde(default)]
    args: Vec<GraphqlArgument>,
}

#[derive(Debug, Deserialize)]
struct GraphqlSchema {
    #[serde(rename = "schemaRef")]
    schema_ref: String,
    #[serde(rename = "operationCount")]
    operation_count: usize,
    operations: Vec<GraphqlOperation>,
}

// ---------------------------------------------------------------------------
// Static catalog — parsed once from embedded JSON
// ---------------------------------------------------------------------------

static CATALOG: OnceLock<Vec<Domain>> = OnceLock::new();

const DOMAIN_FILES: &[(&str, &str)] = &[
    (
        "bulk-operations",
        include_str!("../../schemas/api/bulk-operations.json"),
    ),
    (
        "businesses",
        include_str!("../../schemas/api/businesses.json"),
    ),
    (
        "catalog-products",
        include_str!("../../schemas/api/catalog-products.json"),
    ),
    ("channels", include_str!("../../schemas/api/channels.json")),
    (
        "chargebacks",
        include_str!("../../schemas/api/chargebacks.json"),
    ),
    (
        "customer-profiles",
        include_str!("../../schemas/api/customer-profiles.json"),
    ),
    (
        "fulfillments",
        include_str!("../../schemas/api/fulfillments.json"),
    ),
    (
        "location-addresses",
        include_str!("../../schemas/api/location-addresses.json"),
    ),
    (
        "metafields",
        include_str!("../../schemas/api/metafields.json"),
    ),
    (
        "onboarding",
        include_str!("../../schemas/api/onboarding.json"),
    ),
    ("orders", include_str!("../../schemas/api/orders.json")),
    (
        "payment-requests",
        include_str!("../../schemas/api/payment-requests.json"),
    ),
    ("payments", include_str!("../../schemas/api/payments.json")),
    (
        "price-adjustments",
        include_str!("../../schemas/api/price-adjustments.json"),
    ),
    (
        "recommendations",
        include_str!("../../schemas/api/recommendations.json"),
    ),
    ("shipping", include_str!("../../schemas/api/shipping.json")),
    ("stores", include_str!("../../schemas/api/stores.json")),
    (
        "subscriptions",
        include_str!("../../schemas/api/subscriptions.json"),
    ),
    ("taxes", include_str!("../../schemas/api/taxes.json")),
    (
        "transactions",
        include_str!("../../schemas/api/transactions.json"),
    ),
    (
        "hosting-nodejs",
        include_str!("../../schemas/api/hosting-nodejs.json"),
    ),
    ("domains", include_str!("../../schemas/api/domains.json")),
];

fn catalog() -> &'static [Domain] {
    CATALOG.get_or_init(|| {
        let mut domains: Vec<Domain> = DOMAIN_FILES
            .iter()
            .filter_map(|(_, src)| serde_json::from_str::<Domain>(src).ok())
            .collect();
        // Sorted once here (not per-listing) so every consumer — `api domain
        // list`, `api search`, `api operation get` — sees the same stable order.
        domains.sort_by(|a, b| a.name.cmp(&b.name));
        domains
    })
}

/// True if `concrete`'s path segments structurally match `template`'s: a
/// `{param}` segment in `template` matches any single non-empty segment in
/// `concrete` at the same position, every other segment must match
/// literally (case-insensitive). Matches a real request path with path
/// params substituted (e.g. `/stores/abc123/orders`) against the catalog's
/// templated path (`/stores/{storeId}/orders`), which neither exact nor
/// substring matching can do — a concrete path never literally contains the
/// `{storeId}` placeholder.
fn path_matches_template(template: &str, concrete: &str) -> bool {
    let template_segs: Vec<&str> = template.trim_matches('/').split('/').collect();
    let concrete_segs: Vec<&str> = concrete.trim_matches('/').split('/').collect();
    template_segs.len() == concrete_segs.len()
        && template_segs
            .iter()
            .zip(concrete_segs.iter())
            .all(|(t, c)| {
                (t.starts_with('{') && t.ends_with('}') && !c.is_empty())
                    || t.eq_ignore_ascii_case(c)
            })
}

fn find_endpoint<'a>(catalog: &'a [Domain], query: &str) -> Option<(&'a Domain, &'a Endpoint)> {
    let q = query.to_lowercase();
    catalog.iter().find_map(|domain| {
        domain.endpoints.iter().find_map(|ep| {
            if ep.operation_id.to_lowercase() == q
                || ep.path.to_lowercase() == q
                || path_matches_template(&ep.path, query)
                || ep.path.to_lowercase().contains(&q)
            {
                Some((domain, ep))
            } else {
                None
            }
        })
    })
}

/// Exact (non-fuzzy) endpoint lookup by operationId, full path equality, or
/// a concrete path against a templated catalog path (e.g. `/stores/abc123`
/// against `/stores/{storeId}`), optionally narrowed to a specific HTTP
/// method. Used by `api operation get`'s primary resolution step, distinct from
/// `find_endpoint`'s looser substring-`contains` match (which `api call`
/// still relies on for scope resolution and is left untouched).
///
/// Returns every match, not just the first: many catalog paths are shared by
/// several endpoints that only differ by method (e.g. `GET`/`POST` on the
/// same collection endpoint), so without `--method` there can genuinely be
/// more than one exact match — the caller decides what to do with that
/// (typically: 1 match resolves transparently, >1 is treated the same as an
/// ambiguous fuzzy match).
fn find_endpoint_exact<'a>(
    catalog: &'a [Domain],
    query: &str,
    method: Option<&str>,
) -> Vec<(&'a Domain, &'a Endpoint)> {
    let q = query.to_lowercase();
    catalog
        .iter()
        .flat_map(|domain| {
            let q = q.clone();
            domain.endpoints.iter().filter_map(move |ep| {
                let path_matches = ep.operation_id.to_lowercase() == q
                    || ep.path.to_lowercase() == q
                    || path_matches_template(&ep.path, query);
                let method_matches = method.is_none_or(|m| ep.method.eq_ignore_ascii_case(m));
                (path_matches && method_matches).then_some((domain, ep))
            })
        })
        .collect()
}

/// Builds an "ambiguous match" error for `resolve_operation`'s two >1-hit
/// branches — an exact path shared by several HTTP methods, or a fuzzy
/// query hitting several unrelated endpoints. Every candidate is formatted
/// as a runnable `gddy api operation get <path> --method <method>` line and
/// joined into the error's `fix` field: cli-engine's error envelope has no
/// hook today for a `DetailedError` to attach structured `next_actions`
/// (`build_error_envelope` always sets `next_actions: Vec::new()`), so a
/// formatted `fix` string is the richest thing available until that's
/// closed upstream (filed as a follow-up ticket, matching DEVEX-968/972/981
/// this session).
fn ambiguous_operation_error(query: &str, hits: &[(&Domain, &Endpoint)]) -> CliCoreError {
    let candidates = hits
        .iter()
        .map(|(_, ep)| {
            format!(
                "gddy api operation get {} --method {}  # {}",
                ep.path, ep.method, ep.summary
            )
        })
        .collect::<Vec<_>>()
        .join("\n  ");
    crate::error::GddyError::ambiguous(format!(
        "'{query}' matches {} operations. Be more specific:",
        hits.len()
    ))
    .with_fix(format!("Run one of:\n  {candidates}"))
    .into_cli_error()
}

/// Resolves a `--operation`/positional operation query against the catalog
/// — shared by `operation get` and every command scoped by `--operation`
/// (`parameter`/`response` list and get, `schema get`), since they all need
/// the same exact/fuzzy/ambiguous cascade `operation_get_command` already
/// implemented before this existed. Both "no match" and "more than one
/// match" are errors — there's no single operation to act on either way.
fn resolve_operation<'a>(
    catalog: &'a [Domain],
    query: &str,
    method_filter: Option<&str>,
) -> Result<(&'a Domain, &'a Endpoint), CliCoreError> {
    let exact_hits = find_endpoint_exact(catalog, query, method_filter);
    match exact_hits.len() {
        1 => Ok((exact_hits[0].0, exact_hits[0].1)),
        0 => {
            let hits = search_endpoints(catalog, query);
            match hits.len() {
                0 => Err(crate::error::GddyError::not_found(format!(
                    "no operation found matching '{query}' — try `gddy api search {query}`"
                ))
                .with_fix(format!(
                    "Run: gddy api search {query} or gddy api operation list"
                ))
                .into_cli_error()),
                1 => Ok((hits[0].0, hits[0].1)),
                _ => Err(ambiguous_operation_error(query, &hits)),
            }
        }
        _ => Err(ambiguous_operation_error(query, &exact_hits)),
    }
}

/// Split a `--header` value of the form `KEY:VALUE` into trimmed parts.
/// Splits on the first colon only, so values may themselves contain colons
/// (e.g. a URL). Returns None when there is no colon.
fn split_header(raw: &str) -> Option<(&str, &str)> {
    raw.split_once(':').map(|(k, v)| (k.trim(), v.trim()))
}

/// Whether an HTTP method mutates server state, for `--dry-run` gating.
/// `call`'s tier is fixed at spec-build time, but the method is a runtime
/// arg — this decides per-invocation whether `--dry-run` should actually
/// short-circuit the request. Case-insensitive.
fn is_mutating_method(method: &str) -> bool {
    !(method.eq_ignore_ascii_case("GET") || method.eq_ignore_ascii_case("HEAD"))
}

/// Parse a response body as JSON when possible, otherwise preserve it as raw
/// UTF-8 text. A non-JSON body (plain text / HTML error page) must not be
/// silently dropped to `null` — only a truly empty or binary body becomes null.
fn parse_response_body(bytes: &[u8]) -> Value {
    match serde_json::from_slice::<Value>(bytes) {
        Ok(v) => v,
        Err(_) => match std::str::from_utf8(bytes) {
            Ok(s) if !s.is_empty() => json!(s),
            _ => Value::Null,
        },
    }
}

/// A non-empty top-level GraphQL `errors` array, if present. GraphQL endpoints
/// return HTTP 200 even on failure and carry the failure here.
fn graphql_errors(body: &Value) -> Option<&Vec<Value>> {
    body.get("errors")
        .and_then(|e| e.as_array())
        .filter(|a| !a.is_empty())
}

/// Union of user-supplied `--scope` flags and a matched endpoint's declared
/// scopes, order-preserving and de-duplicated (flags first).
fn merge_required_scopes(flag_scopes: Vec<String>, endpoint_scopes: &[String]) -> Vec<String> {
    let mut required: Vec<String> = Vec::new();
    // De-dup across both sources (a user can repeat `--scope`), flags first.
    for scope in flag_scopes
        .into_iter()
        .chain(endpoint_scopes.iter().cloned())
    {
        if !required.contains(&scope) {
            required.push(scope);
        }
    }
    required
}

fn search_endpoints<'a>(catalog: &'a [Domain], query: &str) -> Vec<(&'a Domain, &'a Endpoint)> {
    let q = query.to_lowercase();
    catalog
        .iter()
        .flat_map(|domain| {
            let q = q.clone();
            domain.endpoints.iter().filter_map(move |ep| {
                let haystack = format!(
                    "{} {} {} {}",
                    ep.operation_id.to_lowercase(),
                    ep.path.to_lowercase(),
                    ep.summary.to_lowercase(),
                    ep.description.to_lowercase(),
                );
                // Only scan GraphQL operations (up to 149 on some domains)
                // when the core fields didn't already match, and stop at the
                // first hit instead of concatenating every operation into one
                // string up front.
                let matches = haystack.contains(&q)
                    || ep.graphql.as_ref().is_some_and(|g| {
                        g.operations.iter().any(|op| {
                            format!("{} {}", op.kind, op.name)
                                .to_lowercase()
                                .contains(&q)
                        })
                    });
                if matches { Some((domain, ep)) } else { None }
            })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Schema summarization — condenses a raw JSON-schema fragment down to
// top-level property names/types for `api operation get`, so a caller sees a
// scannable summary instead of a full nested schema dump. Ported from the
// original TypeScript CLI's `summarizeSchema`/`schemaTypeLabel` family.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
struct SchemaSummaryProperty {
    name: String,
    #[serde(rename = "type")]
    prop_type: String,
    required: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    format: Option<String>,
    #[serde(rename = "enum", skip_serializing_if = "Option::is_none")]
    enum_values: Option<Vec<Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    items: Option<String>,
}

/// Max `$ref` hops to follow when resolving a schema fragment against a
/// domain's `$defs` — bounds a def-to-def cycle (e.g. A refers to B refers
/// back to A) rather than looping forever.
const MAX_REF_DEPTH: u8 = 5;

/// Follows a `{"$ref": "#/$defs/Name"}` pointer against `defs`, transparently
/// substituting the referenced schema, up to `MAX_REF_DEPTH` hops (a def can
/// itself be a `$ref` to another def). Returns `schema` unchanged if it
/// isn't a `$ref`, or the last-resolved schema if the chain doesn't resolve
/// (unknown def name, external non-`#/$defs/` ref, or depth exceeded) —
/// callers fall back to rendering that as an unresolved `ref(...)`.
fn resolve_schema_ref<'a>(schema: &'a Value, defs: &'a Map<String, Value>) -> &'a Value {
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
fn schema_type_label(schema: &Value, defs: &Map<String, Value>) -> String {
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
fn schema_base_type_label(schema: &Value, defs: &Map<String, Value>) -> String {
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

fn summarize_schema(
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

// ---------------------------------------------------------------------------
// Full schema trees and their ids (DEVEX-967's `api schema get <id>`).
//
// Unlike `summarize_schema` above, which stops at one level and collapses a
// nested object/array to a compact string via `schema_type_label`,
// `build_schema_tree` recurses through every level — `api schema get` never
// truncates (see `crate::summary::Summary`), so it always returns the
// complete tree.
//
// A schema's id is computed here at runtime, not minted by
// `generate-api-catalog`: a `$ref` schema already has a stable name (its
// existing `$defs` key), and an inline/anonymous schema's dotted path is a
// pure function of (operationId, where it sits in the operation), so
// there's nothing to persist ahead of time.
// ---------------------------------------------------------------------------

/// Where a schema sits within an operation — used by `compute_schema_id`
/// when the schema has no `$ref` name of its own, and by `parse_schema_id`
/// to resolve an id back to a concrete schema.
#[allow(dead_code)] // wired in once the `schema`/`parameter`/`response` commands land
#[derive(Debug, Clone, PartialEq, Eq)]
enum SchemaLocation {
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
#[allow(dead_code)] // wired in once the `schema`/`parameter`/`response` commands land
const SCHEMA_ID_KEYWORDS: [&str; 3] = ["parameters", "responses", "requestBody"];

/// Computes a stable id for `schema`: its `$defs` key if it's a `$ref`, or a
/// dotted path rooted at `operation_id` if it's inline/anonymous.
#[allow(dead_code)] // wired in once the `schema`/`parameter`/`response` commands land
fn compute_schema_id(operation_id: &str, location: &SchemaLocation, schema: &Value) -> String {
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
fn scalar_schema_type(raw_schema: Option<&Value>, defs: &Map<String, Value>) -> Option<String> {
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
fn describe_schema(
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
#[allow(dead_code)] // wired in once the `schema`/`parameter`/`response` commands land
fn parse_schema_id(id: &str) -> Option<(String, SchemaLocation)> {
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
#[allow(dead_code)] // wired in once the `schema` command lands
#[derive(Debug, Clone, Serialize)]
struct SchemaNode {
    name: String,
    #[serde(rename = "type")]
    node_type: String,
    required: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    format: Option<String>,
    #[serde(rename = "enum", skip_serializing_if = "Option::is_none")]
    enum_values: Option<Vec<Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    properties: Option<Vec<SchemaNode>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    items: Option<Box<SchemaNode>>,
}

/// Recursively expands `schema` into a `SchemaNode` tree. Reuses
/// `resolve_schema_ref`/`MAX_REF_DEPTH` unchanged for `$ref`-cycle bounding
/// — that bound is orthogonal to how deep object/array nesting itself is
/// allowed to go, which is intentionally unbounded here.
#[allow(dead_code)] // wired in once the `schema` command lands
fn build_schema_tree(name: &str, schema: &Value, defs: &Map<String, Value>) -> SchemaNode {
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

// ---------------------------------------------------------------------------
// `api parameter`/`api response` — first-class entities scoped by
// `--operation`, backing `api parameter list/get` and `api response
// list/get`. `api operation get` (below) summarizes the same rows for its
// own preview.
// ---------------------------------------------------------------------------

/// Breadth cap on a parameter's/response's *embedded* schema preview — the
/// full, untruncated tree is always one `api schema get <id>` away (see
/// `build_schema_tree`), so this only bounds how much of it gets inlined
/// here.
const SCHEMA_PROPERTY_PREVIEW_CAP: usize = 20;

/// Default page size for `api parameter list`/`api response list` (via
/// `CommandSpec::with_pagination`) when neither `--limit` nor `--offset` is
/// passed, and for `api operation get`'s own embedded parameter/response
/// previews (which aren't paginated themselves — see `Summary::capped`
/// there — but use the same "how many rows to show inline" size).
const OPERATION_CHILD_LIST_DEFAULT_LIMIT: usize = 20;

/// Upper bound a user can request with `--limit` on `api parameter list`/
/// `api response list`. Generous relative to the real catalog (26
/// parameters, 11 responses is the current max of either), just to guard
/// against a client asking for something absurd.
const OPERATION_CHILD_LIST_MAX_LIMIT: i64 = 200;

#[derive(Debug, Clone, Serialize)]
struct ParameterSummary {
    name: String,

    #[serde(rename = "in")]
    location: String,

    required: bool,

    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    schema: Option<Summary<SchemaSummaryProperty>>,

    /// Id for `api schema get`, present whenever this parameter's schema has
    /// something to drill into — an object with properties, or a `$ref`
    /// (which might resolve to one) — regardless of whether the embedded
    /// preview above happens to be truncated, so a caller always knows what
    /// shape a parameter (most importantly `body`) expects. `None` for a
    /// bare inline scalar (see `scalar_type` below), where drilling in would
    /// just echo the same type back.
    #[serde(rename = "schemaId", skip_serializing_if = "Option::is_none")]
    schema_id: Option<String>,

    /// The type to show alongside `schema_id`/`schema`: the full label
    /// (e.g. `"string(uuid)"`, `"array<string>"`) when `schema_id` is
    /// absent (a bare scalar — the label is the whole story), or just the
    /// coarse base type (e.g. `"object"`) when `schema_id` *is* present,
    /// since the full nested detail is a `schema get` away instead of being
    /// repeated here. Always present whenever there's a schema at all — see
    /// `describe_schema`.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    scalar_type: Option<String>,
}

/// Every parameter on `ep`, plus — if it has a request body — a synthetic
/// `{name: "body", in: "body"}` row. In the catalog's flattened shape
/// (`ep.request_body`: `required`/`contentType`/`schema`, no `name`/`in` of
/// its own), a request body is structurally just a parameter missing a
/// name, so this is the one place both are normalized into the same shape.
fn summarize_parameters(ep: &Endpoint, defs: &Map<String, Value>) -> Vec<ParameterSummary> {
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
fn find_parameter_schema<'a>(ep: &'a Endpoint, name: &str) -> Option<(&'a Value, SchemaLocation)> {
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
struct ResponseRow {
    status: String,
    description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    schema: Option<Summary<SchemaSummaryProperty>>,
    /// See `ParameterSummary::schema_id` — same "nothing to drill into"
    /// exception for a bare inline scalar.
    #[serde(rename = "schemaId", skip_serializing_if = "Option::is_none")]
    schema_id: Option<String>,
    /// See `ParameterSummary::scalar_type`.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    scalar_type: Option<String>,
}

/// Every response on `ep`, sorted by status for a stable listing order
/// (`ep.responses` is parsed straight from JSON and carries no ordering
/// guarantee of its own).
fn response_rows(ep: &Endpoint, defs: &Map<String, Value>) -> Vec<ResponseRow> {
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
fn to_json(value: impl Serialize) -> Result<Value, CliCoreError> {
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

fn summarize_graphql_schema(graphql: &GraphqlSchema) -> Value {
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
                "deprecated": op.deprecated,
                "deprecationReason": op.deprecation_reason,
                "args": op.args.iter().map(|a| json!({
                    "name": a.name,
                    "type": a.arg_type,
                    "required": a.required,
                    "defaultValue": a.default_value,
                })).collect::<Vec<_>>(),
            })
        })
        .collect();
    json!({
        "schemaRef": graphql.schema_ref,
        "operationCount": graphql.operation_count,
        "queryCount": query_count,
        "mutationCount": mutation_count,
        "operations": operations,
    })
}

// ---------------------------------------------------------------------------
// Module
// ---------------------------------------------------------------------------

pub fn module() -> Module {
    Module::new("API", |_ctx| {
        RuntimeGroupSpec::new(
            GroupSpec::new("api", "Explore and call GoDaddy API endpoints").with_long(
                "Browse the GoDaddy API catalog and make authenticated requests \
                     against any endpoint. Use `api domain list` / `api operation list` to \
                     discover available operations, `api operation get` to inspect parameters, \
                     and `api call` to execute a request with automatic OAuth scope handling.",
            ),
        )
        .with_group(
            RuntimeGroupSpec::new(GroupSpec::new("domain", "Browse API domains").with_long(
                "Browse the top-level API domains available in the embedded catalog. \
                     Each domain groups a related set of endpoints under a shared base URL. \
                     Use `api operation list --domain <domain>` to see the endpoints \
                     within a specific domain.",
            ))
            .with_command(domain_list_command()),
        )
        .with_group(
            RuntimeGroupSpec::new(
                GroupSpec::new("operation", "Browse and inspect API operations").with_long(
                    "Browse the operations within a single API domain. \
                     Use `api domain list` first to find available domain names, \
                     then `api operation get <operationId>` to inspect full parameter \
                     and schema details for an individual operation.",
                ),
            )
            .with_command(operation_list_command())
            .with_command(operation_get_command()),
        )
        .with_group(
            RuntimeGroupSpec::new(
                GroupSpec::new("parameter", "Inspect an operation's parameters").with_long(
                    "Inspect the parameters of a single operation, scoped by `--operation`. \
                     A request body counts as a parameter here too, under the synthetic name \
                     `body`.",
                ),
            )
            .with_command(parameter_list_command())
            .with_command(parameter_get_command()),
        )
        .with_group(
            RuntimeGroupSpec::new(
                GroupSpec::new("response", "Inspect an operation's responses").with_long(
                    "Inspect the responses of a single operation, scoped by `--operation`.",
                ),
            )
            .with_command(response_list_command())
            .with_command(response_get_command()),
        )
        .with_group(
            RuntimeGroupSpec::new(GroupSpec::new(
                "schema",
                "Inspect a named or inline API schema",
            ))
            .with_command(schema_get_command()),
        )
        .with_command(search_command())
        .with_command(call_command())
    })
    .with_guides_from_markdown([(
        "api-explorer.md",
        include_bytes!("guides/api-explorer.md").as_slice(),
    )])
}

fn domain_list_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_with_context(
        CommandSpec::new("list", "List all API domains")
            .with_long(
                "Lists every API domain in the embedded catalog, together with the number \
                 of endpoints and base URL for each. No authentication is required. \
                 Use `api operation list --domain <domain>` to drill into a specific domain, \
                 or `api search <query>` to find endpoints across all domains at once.",
            )
            .with_system("api")
            .with_tier(Tier::Read)
            .no_auth(true)
            .with_default_fields("domain,title,endpoints,baseUrl")
            .with_output_schema::<ApiDomain>(),
        |ctx| async move {
            let catalog = catalog();
            let domains: Vec<Value> = catalog
                .iter()
                .map(|d| {
                    let base_url = crate::environments::resolve_catalog_base_url(
                        &d.name,
                        &d.base_url,
                        &ctx.middleware.env,
                    );
                    json!({
                        "domain": d.name,
                        "title": d.title,
                        "description": d.description,
                        "endpoints": d.endpoints.len(),
                        "baseUrl": base_url,
                    })
                })
                .collect();
            Ok(CommandResult::new(json!(domains)).with_next_actions(vec![
                next_action(
                    "api operation list --domain <domain>",
                    "List endpoints in a specific domain",
                )
                .with_param("domain", NextActionParam::required()),
                next_action("api search <query>", "Search across all endpoints"),
            ]))
        },
    )
}

#[derive(Debug, Clone, clap::Args)]
struct OperationListArgs {
    /// API domain whose endpoints to list (see `api domain list`).
    #[arg(long, value_name = "DOMAIN")]
    domain: String,
}

fn operation_list_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed::<OperationListArgs, _, _, _>(
        CommandSpec::from_args::<OperationListArgs>("list", "List operations within an API domain")
            .with_long(
                "Lists every operation in one API domain, showing the operation ID, HTTP \
                 method, path, and summary. Use `api domain list` to find available domain \
                 names, `api operation get <operationId>` to view full parameter details, and \
                 `api call <path>` to execute a request.",
            )
            .with_system("api")
            .with_tier(Tier::Read)
            .no_auth(true)
            .with_default_fields("operationId,method,path,summary")
            .with_output_schema::<ApiDomainEndpoint>(),
        |_cred, args: OperationListArgs| async move {
            let catalog = catalog();
            let domain_filter = args.domain.as_str();
            let domain = catalog
                .iter()
                .find(|d| d.name == domain_filter)
                .ok_or_else(|| {
                    crate::error::GddyError::not_found(format!(
                        "domain '{domain_filter}' not found"
                    ))
                    .with_fix("Run: gddy api domain list")
                    .into_cli_error()
                })?;
            let endpoints: Vec<Value> = domain
                .endpoints
                .iter()
                .map(|ep| {
                    json!({
                        "operationId": ep.operation_id,
                        "method": ep.method,
                        "path": ep.path,
                        "summary": ep.summary,
                        "scopes": ep.scopes,
                        "graphqlOperations": ep.graphql.as_ref().map(|g| g.operation_count),
                    })
                })
                .collect();
            Ok(CommandResult::new(json!(endpoints)).with_next_actions(vec![
                next_action(
                    "api operation get <operation>",
                    "Get full details for an operation",
                )
                .with_param("operation", NextActionParam::required()),
            ]))
        },
    )
}

#[derive(Debug, Clone, clap::Args)]
struct OperationGetArgs {
    /// Operation ID (e.g. createOrder) or path fragment (e.g. /v1/commerce/orders).
    #[arg(value_name = "OPERATION")]
    operation: String,

    /// Filter to a specific HTTP method (GET, POST, PUT, PATCH, DELETE).
    #[arg(long, short = 'm', value_name = "METHOD")]
    method: Option<String>,
}

fn operation_get_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<OperationGetArgs, _, _, _>(
        CommandSpec::from_args::<OperationGetArgs>(
            "get",
            "Show schema and parameters for an operation",
        )
        .with_long(
            "Shows the full details of one API operation: HTTP method, path, required and \
             optional parameters, request body schema, response shapes, and declared OAuth \
             scopes. Accepts an operation ID (e.g. createOrder) or a path fragment \
             (e.g. /v1/commerce/orders). No authentication is required.",
        )
        .with_system("api")
        .with_tier(Tier::Read)
        .no_auth(true)
        .with_output_schema::<ApiOperation>()
        // `parameters.items` is a list of objects, so it renders as an
        // indented child table instead of a raw JSON dump. The dotted path
        // reaches through the `Summary<T>` envelope now wrapping it
        // (cli-engine 0.7's `TableColumn::field` supports this). No
        // `message`/`matches` columns here — an ambiguous or unresolved
        // query is a hard error (see `resolve_operation`/
        // `ambiguous_operation_error`), not a second shape this view has to
        // cover, so this only ever needs to render one resolved operation.
        .with_view(vec![
            TableColumn::new("domain", "Domain"),
            TableColumn::new("operationId", "Operation ID"),
            TableColumn::new("method", "Method"),
            TableColumn::new("path", "Path"),
            TableColumn::new("summary", "Summary"),
            TableColumn::new("parameters.items", "Parameters").nested(vec![
                TableColumn::new("name", "Name"),
                TableColumn::new("in", "In"),
                TableColumn::new("required", "Required"),
                TableColumn::new("type", "Type"),
                TableColumn::new("schemaId", "Schema ID"),
                TableColumn::new("description", "Description"),
            ]),
            TableColumn::new("responses.items", "Responses").nested(vec![
                TableColumn::new("status", "Status"),
                TableColumn::new("type", "Type"),
                TableColumn::new("schemaId", "Schema ID"),
                TableColumn::new("description", "Description"),
            ]),
        ]),
        |ctx, args: OperationGetArgs| async move {
            let query = args.operation.as_str();
            let method_filter = args.method.map(|m| m.to_uppercase());
            let catalog = catalog();

            // Exact operationId/path/template match (optionally narrowed by
            // --method) first; a miss falls back to fuzzy substring search,
            // which ignores --method entirely (matches the original CLI).
            // Either step can legitimately produce more than one candidate
            // (e.g. GET/POST sharing a path with no --method given) — both
            // are treated as the same kind of ambiguity.
            let (domain, ep) = resolve_operation(catalog, query, method_filter.as_deref())?;

            // Env-aware base URL, matching `domain_list_command`/`call_command`
            // — the catalog's `baseUrl` is a static, prod-shaped value; this
            // resolves it against the active environment the same way an
            // actual `api call` to this endpoint would.
            let base_url = crate::environments::resolve_catalog_base_url(
                &domain.name,
                &domain.base_url,
                &ctx.middleware.env,
            );

            // Summarized and capped, not inlined in full — a truncated
            // preview here links to the standalone `parameter list`/
            // `response list` (which do their own, independent truncation)
            // rather than dumping everything inline.
            let param_summary = Summary::capped(
                summarize_parameters(ep, &domain.defs),
                OPERATION_CHILD_LIST_DEFAULT_LIMIT,
            );
            let response_summary = Summary::capped(
                response_rows(ep, &domain.defs),
                OPERATION_CHILD_LIST_DEFAULT_LIMIT,
            );

            // Strip `base_url`'s scheme+host generically (not a hard-coded
            // prod hostname) so `fullPath` stays a hostless path prefix +
            // endpoint path consistently across every environment, not just
            // prod — `resolve_catalog_base_url` rewrites the host per env
            // (e.g. `api.ote-godaddy.com`), which a literal prod-host strip
            // would silently leave un-stripped.
            let full_path = {
                let without_scheme = base_url
                    .split_once("://")
                    .map_or(base_url.as_str(), |(_, rest)| rest);
                let path_prefix = without_scheme
                    .find('/')
                    .map_or("", |i| &without_scheme[i..]);
                format!("{path_prefix}{}", ep.path)
            };

            let mut next_actions = vec![next_action(
                format!("api call {} --method {}", ep.path, ep.method),
                "Make an authenticated call to this endpoint",
            )];
            next_actions.extend(
                param_summary.next_action_if_truncated(
                    next_action(
                        "api parameter list --operation <operation>",
                        "See all parameters",
                    )
                    .with_param("operation", required_value(ep.operation_id.clone())),
                ),
            );
            next_actions.extend(
                response_summary.next_action_if_truncated(
                    next_action(
                        "api response list --operation <operation>",
                        "See all responses",
                    )
                    .with_param("operation", required_value(ep.operation_id.clone())),
                ),
            );
            // A schema id shown in the table but not echoed anywhere in
            // `next_actions` is a dead end unless the caller already knows
            // `api schema get <id>` exists. One generic pointer to that
            // command covers every parameter/response with a `schemaId` at
            // once, rather than a same-command line repeated per row (most
            // schema ids on one operation are typically shared, e.g. a
            // common `Error` response schema across several status codes).
            if param_summary
                .items
                .iter()
                .any(|param| param.schema_id.is_some())
                || response_summary
                    .items
                    .iter()
                    .any(|resp| resp.schema_id.is_some())
            {
                next_actions.push(
                    next_action(
                        "api schema get <id>",
                        "See a parameter's or response's full schema — use its Schema ID from the table above",
                    )
                    .with_param("id", NextActionParam::required()),
                );
            }

            Ok(CommandResult::new(json!({
                "domain": domain.name,
                "baseUrl": base_url,
                "operationId": ep.operation_id,
                "method": ep.method,
                "path": ep.path,
                "fullPath": full_path,
                "summary": ep.summary,
                "description": ep.description,
                "parameters": param_summary,
                "responses": response_summary,
                "scopes": ep.scopes,
                "graphql": ep.graphql.as_ref().map(summarize_graphql_schema),
            }))
            .with_next_actions(next_actions))
        },
    )
}

#[derive(Debug, Clone, clap::Args)]
struct SearchArgs {
    /// Search term (matches path, operationId, summary, description).
    #[arg(value_name = "QUERY")]
    query: String,
}

fn search_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed::<SearchArgs, _, _, _>(
        CommandSpec::from_args::<SearchArgs>("search", "Search API endpoints by keyword")
            .with_long(
                "Full-text searches across all API domains, matching against operation IDs, \
                 paths, summaries, and descriptions. Returns matching endpoints with domain, \
                 method, path, and summary. No authentication is required. Use \
                 `api operation get <operationId>` to inspect a result in full detail.",
            )
            .with_system("api")
            .with_tier(Tier::Read)
            .no_auth(true)
            .with_default_fields("domain,method,path,summary")
            .with_output_schema::<ApiEndpoint>(),
        |_cred, args: SearchArgs| async move {
            let hits = search_endpoints(catalog(), &args.query);
            if hits.is_empty() {
                return Ok(CommandResult::new(json!([])));
            }
            let results: Vec<Value> = hits
                .iter()
                .map(|(domain, ep)| {
                    json!({
                        "domain": domain.name,
                        "operationId": ep.operation_id,
                        "method": ep.method,
                        "path": ep.path,
                        "summary": ep.summary,
                        "scopes": ep.scopes,
                        "graphqlOperations": ep.graphql.as_ref().map(|g| g.operation_count),
                    })
                })
                .collect();
            Ok(CommandResult::new(json!(results)).with_next_actions(vec![
                next_action(
                    "api operation get <operation>",
                    "Get full details for a result",
                )
                .with_param("operation", NextActionParam::required()),
            ]))
        },
    )
}

#[derive(Debug, Clone, clap::Args)]
struct CallArgs {
    /// Relative API path (e.g. /v1/commerce/stores/{storeId}/orders).
    #[arg(value_name = "ENDPOINT")]
    endpoint: String,

    /// HTTP method.
    #[arg(long, short = 'X', value_name = "METHOD", default_value = "GET")]
    method: String,

    /// Request body as raw JSON string.
    #[arg(long, short = 'd', value_name = "JSON")]
    body: Option<String>,

    /// Add a field to the request body (key=value, repeatable).
    #[arg(long, short = 'f', value_name = "KEY=VALUE")]
    field: Vec<String>,

    /// Read request body from a JSON file.
    #[arg(long, short = 'F', value_name = "PATH")]
    file: Option<String>,

    /// Extra request headers.
    #[arg(long, short = 'H', value_name = "KEY:VALUE")]
    header: Vec<String>,

    /// Include response headers in output.
    #[arg(long, short = 'i')]
    include: bool,

    /// Additional required OAuth scope(s), merged with the endpoint's.
    // One value per occurrence, repeatable (`--scope a --scope b`) — a `Vec`
    // field defaults to append-style, not `num_args(1..)`, so it can't
    // greedily consume the ENDPOINT positional either.
    #[arg(long, short = 's', value_name = "SCOPE")]
    scope: Vec<String>,
}

fn call_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<CallArgs, _, _, _>(
        CommandSpec::from_args::<CallArgs>("call", "Make an authenticated API request")
            .with_long(
                "Executes an authenticated HTTP request against any GoDaddy API endpoint. \
                 Required OAuth scopes are resolved automatically: the catalog is searched \
                 for the endpoint path and its declared scopes are merged with any explicit \
                 `--scope` flags, then a credential with exactly those scopes is obtained \
                 before the request is sent. Supply the request body as raw JSON (`--body \
                 '{...}'`), as individual fields (`--field key=value`, repeatable), or \
                 from a JSON file (`--file body.json`); `--file` takes precedence over \
                 `--body`, and `--field` values are merged on top of either. Use the global \
                 `--expr`/`--filter` flags (JMESPath) to extract or filter response data, and \
                 `--include` to see response headers alongside the body. Use \
                 `api operation get <operationId>` first to inspect required parameters and scopes.",
            )
            .with_system("api")
            .with_tier(Tier::Mutate)
            .handles_dry_run(true)
            // The dry-run-for-mutating-methods branch never needs a token, so
            // resolution is deferred to the handler (which only calls
            // `credential_with_scopes` on the path that actually sends a
            // request) instead of the engine resolving eagerly beforehand.
            .auth_optional(),
        |ctx, args: CallArgs| async move {
            let endpoint = args.endpoint.as_str();
            let method = args.method.as_str();
            // Validate the method unconditionally, including under
            // `--dry-run` — otherwise a garbage/malformed method (e.g.
            // "POST " with trailing whitespace) would return a successful
            // dry-run preview even though a real run would reject it here.
            let parsed_method: reqwest::Method = method.parse().map_err(|_| {
                crate::error::GddyError::validation(format!("invalid HTTP method: {method}"))
                    .into_cli_error()
            })?;

            // Validate unconditionally, including under `--dry-run` (same
            // rationale as the method check above). `find_endpoint` below can
            // match a raw operationId, but the request URL is always built
            // from this literal `endpoint` string, not the matched catalog
            // path — accepting a bare operationId here would silently build
            // an invalid URL (e.g. `https://.../listFulfillments`).
            if !endpoint.starts_with('/') {
                return Err(crate::error::GddyError::validation(format!(
                    "endpoint must be a URL path starting with '/', not {endpoint:?} — \
                     use `api operation get {endpoint}` to find the concrete path"
                ))
                .into_cli_error());
            }

            // `--dry-run` is statically tagged `Tier::Mutate` since the method
            // is only known at runtime, but a GET/HEAD is safe to actually run
            // (and more useful previewed as real data than as a generic
            // string) — only short-circuit for methods that would mutate.
            if ctx.dry_run() && is_mutating_method(method) {
                return Ok(CommandResult::new(json!({
                    "command": "api:call",
                    "action": "dry-run: would execute",
                    "method": method,
                    "endpoint": endpoint,
                }))
                .with_dry_run());
            }

            // Best-effort match against the embedded catalog: a concrete request
            // path may not match a templated catalog path, in which case scopes
            // fall back to just --scope and the base URL falls back to the
            // generic gateway host.
            let matched = find_endpoint(catalog(), endpoint);

            // Required scopes = explicit --scope flags, plus the matched
            // endpoint's declared scopes. These drive OAuth scope step-up at
            // credential time.
            let flag_scopes = args.scope.clone();
            let endpoint_scopes = matched.map(|(_, ep)| ep.scopes.as_slice()).unwrap_or(&[]);
            let required = merge_required_scopes(flag_scopes, endpoint_scopes);

            let token = ctx.credential_with_scopes(&required).await?.token;
            let base_url = match matched {
                Some((domain, _)) => crate::environments::resolve_catalog_base_url(
                    &domain.name,
                    &domain.base_url,
                    &ctx.middleware.env,
                ),
                None => crate::application::client::api_url_for_env(&ctx.middleware.env)?,
            };
            let url = format!("{base_url}{endpoint}");

            // Build request body: -F file > -d body, then merge -f fields on top
            let mut request_body: Option<Value> = None;

            if let Some(file_path) = args.file.as_deref() {
                let content = std::fs::read_to_string(file_path).map_err(|e| {
                    crate::error::GddyError::validation(format!(
                        "failed to read file '{file_path}': {e}"
                    ))
                    .into_cli_error()
                })?;
                request_body = Some(serde_json::from_str(&content).map_err(|e| {
                    crate::error::GddyError::validation(format!(
                        "invalid JSON in '{file_path}': {e}"
                    ))
                    .into_cli_error()
                })?);
            } else if let Some(body_str) = args.body.as_deref() {
                request_body = Some(serde_json::from_str(body_str).map_err(|e| {
                    crate::error::GddyError::validation(format!("invalid JSON body: {e}"))
                        .into_cli_error()
                })?);
            }

            let fields = &args.field;
            if !fields.is_empty() {
                let body = request_body.get_or_insert_with(|| json!({}));
                for s in fields {
                    let eq = s.find('=').ok_or_else(|| {
                        crate::error::GddyError::validation(format!(
                            "invalid field format '{s}': expected key=value"
                        ))
                        .into_cli_error()
                    })?;
                    let key = s[..eq].to_owned();
                    let val = s[eq + 1..].to_owned();
                    if let Some(obj) = body.as_object_mut() {
                        obj.insert(key, json!(val));
                    }
                }
            }

            let client = crate::application::client::make_http_client();
            let mut req = client
                .request(parsed_method, &url)
                .bearer_auth(&token)
                .header("x-request-id", uuid::Uuid::new_v4().to_string());

            // Apply user-supplied `--header KEY:VALUE` values (repeatable).
            for h in &args.header {
                let (key, val) = split_header(h).ok_or_else(|| {
                    crate::error::GddyError::validation(format!(
                        "invalid header '{h}': expected KEY:VALUE"
                    ))
                    .into_cli_error()
                })?;
                req = req.header(key, val);
            }

            if let Some(body) = request_body {
                req = req.json(&body);
            }

            let request = req
                .build()
                .map_err(|e| crate::error::GddyError::validation(e.to_string()))?;
            cli_engine::transport::debug_log_reqwest_request(&request);
            let resp = client
                .execute(request)
                .await
                .map_err(|e| crate::error::GddyError::network(e.to_string()))?;

            let status_code = resp.status();
            let status_text = status_code.canonical_reason().unwrap_or("").to_owned();
            let response_headers_raw = resp.headers().clone();
            let include_headers = args.include;
            let body_bytes = resp
                .bytes()
                .await
                .map_err(|e| crate::error::GddyError::network(e.to_string()))?;
            cli_engine::transport::debug_log_reqwest_response(
                status_code,
                &response_headers_raw,
                &body_bytes,
            );

            let status = status_code.as_u16();
            let response_headers: Option<Map<String, Value>> = if include_headers {
                Some(
                    response_headers_raw
                        .iter()
                        .filter_map(|(k, v)| v.to_str().ok().map(|s| (k.to_string(), json!(s))))
                        .collect(),
                )
            } else {
                None
            };

            let body: Value = parse_response_body(&body_bytes);

            // GraphQL endpoints return HTTP 200 even on failure, carrying the error
            // in a top-level `errors` array. Detect the GraphQL commerce surfaces by
            // path and surface those errors instead of reporting a false success.
            let is_graphql = endpoint.contains("graphql") || endpoint.contains("subgraph");
            if is_graphql && let Some(errors) = graphql_errors(&body) {
                return Err(crate::error::GddyError::from_graphql(
                    format!(
                        "GraphQL request returned {} error(s):\n{}",
                        errors.len(),
                        serde_json::to_string_pretty(&json!(errors)).unwrap_or_default(),
                    ),
                    "api",
                )
                .into());
            }

            // Scope step-up already ran up front (the token was requested with
            // `required`). A 403 here means the granted token still lacks a
            // required scope — surface it with a re-login hint.
            if status == 403 && !required.is_empty() {
                // `auth login --scope` is append-style (one value per flag), so
                // repeat the flag rather than space-joining.
                let login_hint = required
                    .iter()
                    .map(|s| format!("--scope {s}"))
                    .collect::<Vec<_>>()
                    .join(" ");
                return Err(crate::error::GddyError::auth(format!(
                    "403 Forbidden — the authorized token is missing required scope(s): {}. \
                     Re-run `gddy auth login {login_hint}` and try again.",
                    required.join(", "),
                ))
                .with_fix(format!("Run: gddy auth login {login_hint}"))
                .into());
            }

            // Any other non-2xx is a failure, not a success envelope. Include the
            // status line and (truncated) response body so the caller sees the detail
            // instead of a success result that happens to carry an error payload.
            if !(200..300).contains(&status) {
                let detail: String = serde_json::to_string_pretty(&body)
                    .unwrap_or_else(|_| body.to_string())
                    .chars()
                    .take(4000)
                    .collect();
                return Err(crate::error::GddyError::from_http(
                    status,
                    format!("{status_text}\n{detail}"),
                    "api",
                )
                .into_cli_error());
            }

            // Identify the call and its outcome in the result envelope.
            let mut result = json!({
                "endpoint": endpoint,
                "method": method,
                "status": status,
                "status_text": status_text,
                "data": body,
            });
            if let Some(headers) = response_headers {
                result["headers"] = Value::Object(headers);
            }

            Ok(CommandResult::new(result))
        },
    )
}

#[derive(Debug, Clone, clap::Args)]
struct ParameterListArgs {
    /// Operation ID to list parameters for (see `api operation list`).
    #[arg(long, value_name = "OPERATION")]
    operation: String,
}

fn parameter_list_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed::<ParameterListArgs, _, _, _>(
        CommandSpec::from_args::<ParameterListArgs>("list", "List an operation's parameters")
            .with_long(
                "Lists every parameter on one operation — query/path/header/cookie \
                 parameters, plus a synthetic `body` row if the operation has a request \
                 body — with a truncated preview of each parameter's schema. Use `api \
                 parameter get <name> --operation <id>` for one parameter's full detail.",
            )
            .with_view(vec![
                TableColumn::new("name", "Name"),
                TableColumn::new("in", "In"),
                TableColumn::new("required", "Required"),
                TableColumn::new("type", "Type"),
                TableColumn::new("schemaId", "Schema ID"),
                TableColumn::new("description", "Description"),
            ])
            .with_system("api")
            .with_tier(Tier::Read)
            .no_auth(true)
            .with_pagination(PaginationConfig {
                default_limit: OPERATION_CHILD_LIST_DEFAULT_LIMIT as i64,
                max_limit: OPERATION_CHILD_LIST_MAX_LIMIT,
            }),
        |_cred, args: ParameterListArgs| async move {
            let operation = args.operation.as_str();
            let catalog = catalog();
            let (domain, ep) = resolve_operation(catalog, operation, None)?;
            // Return every parameter, unsliced: cli-engine 0.8's pipeline
            // slices a bare-array result per this command's own --limit/
            // --offset (registered above via `with_pagination`), surfaces
            // total/offset/limit/count/has_more as top-level `pagination`
            // envelope metadata, and — when `has_more` — appends a "view the
            // next page" next_action itself. No manual truncation or
            // next-page hint needed here anymore.
            let all = summarize_parameters(ep, &domain.defs);
            let next_actions = vec![
                next_action(
                    "api parameter get <name> --operation <operation>",
                    "See one parameter's full detail",
                )
                .with_param("operation", required_value(ep.operation_id.clone())),
            ];
            Ok(CommandResult::new(json!(all)).with_next_actions(next_actions))
        },
    )
}

#[derive(Debug, Clone, clap::Args)]
struct ParameterGetArgs {
    /// Parameter name (or `body` for the request body, if present).
    #[arg(value_name = "NAME")]
    name: String,

    /// Operation ID that owns this parameter (see `api operation list`).
    #[arg(long, value_name = "OPERATION")]
    operation: String,
}

fn parameter_get_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed::<ParameterGetArgs, _, _, _>(
        CommandSpec::from_args::<ParameterGetArgs>(
            "get",
            "Show one operation parameter's full detail",
        )
        .with_long(
            "Shows one parameter's full detail: location, required-ness, description, \
                 and its complete schema (never truncated — see `api schema get`). Also \
                 resolves the synthetic `body` name, if the operation has a request body.",
        )
        .with_system("api")
        .with_tier(Tier::Read)
        .no_auth(true),
        |_cred, args: ParameterGetArgs| async move {
            let name = args.name.as_str();
            let operation = args.operation.as_str();
            let catalog = catalog();
            let (domain, ep) = resolve_operation(catalog, operation, None)?;
            let row = summarize_parameters(ep, &domain.defs)
                .into_iter()
                .find(|p| p.name == name)
                .ok_or_else(|| {
                    crate::error::GddyError::not_found(format!(
                        "parameter '{name}' not found on operation '{}'",
                        ep.operation_id
                    ))
                    .with_fix(format!(
                        "Run: gddy api parameter list --operation {}",
                        ep.operation_id
                    ))
                    .into_cli_error()
                })?;
            let next_actions = row
                .schema_id
                .clone()
                .map(|id| {
                    next_action("api schema get <id>", "See the full parameter schema")
                        .with_param("id", required_value(id))
                })
                .into_iter()
                .collect();
            Ok(CommandResult::new(to_json(row)?).with_next_actions(next_actions))
        },
    )
}

#[derive(Debug, Clone, clap::Args)]
struct ResponseListArgs {
    /// Operation ID to list responses for (see `api operation list`).
    #[arg(long, value_name = "OPERATION")]
    operation: String,
}

fn response_list_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed::<ResponseListArgs, _, _, _>(
        CommandSpec::from_args::<ResponseListArgs>("list", "List an operation's responses")
            .with_long(
                "Lists every response on one operation by status code, with a truncated \
                 preview of each response's schema. Use `api response get <status> \
                 --operation <id>` for one response's full detail.",
            )
            // See `parameter_list_command`'s comment: a declared view avoids
            // cli-engine's alphabetical no-view fallback and gives proper
            // headers. Matches `operation get`'s own `responses.items`
            // nested view's columns.
            .with_view(vec![
                TableColumn::new("status", "Status"),
                TableColumn::new("type", "Type"),
                TableColumn::new("schemaId", "Schema ID"),
                TableColumn::new("description", "Description"),
            ])
            .with_system("api")
            .with_tier(Tier::Read)
            .no_auth(true)
            .with_pagination(PaginationConfig {
                default_limit: OPERATION_CHILD_LIST_DEFAULT_LIMIT as i64,
                max_limit: OPERATION_CHILD_LIST_MAX_LIMIT,
            }),
        |_cred, args: ResponseListArgs| async move {
            let operation = args.operation.as_str();
            let catalog = catalog();
            let (domain, ep) = resolve_operation(catalog, operation, None)?;
            // See parameter_list_command's comment: cli-engine 0.8's
            // pipeline handles slicing, pagination metadata, and the
            // next-page hint for a bare-array result on its own.
            let all = response_rows(ep, &domain.defs);
            let next_actions = vec![
                next_action(
                    "api response get <status> --operation <operation>",
                    "See one response's full detail",
                )
                .with_param("operation", required_value(ep.operation_id.clone())),
            ];
            Ok(CommandResult::new(json!(all)).with_next_actions(next_actions))
        },
    )
}

#[derive(Debug, Clone, clap::Args)]
struct ResponseGetArgs {
    /// HTTP status code (e.g. 200).
    #[arg(value_name = "STATUS")]
    status: String,

    /// Operation ID that owns this response (see `api operation list`).
    #[arg(long, value_name = "OPERATION")]
    operation: String,
}

fn response_get_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed::<ResponseGetArgs, _, _, _>(
        CommandSpec::from_args::<ResponseGetArgs>(
            "get",
            "Show one operation response's full detail",
        )
        .with_long(
            "Shows one response's full detail: description and its complete schema \
                 (never truncated — see `api schema get`).",
        )
        .with_system("api")
        .with_tier(Tier::Read)
        .no_auth(true),
        |_cred, args: ResponseGetArgs| async move {
            let status = args.status.as_str();
            let operation = args.operation.as_str();
            let catalog = catalog();
            let (domain, ep) = resolve_operation(catalog, operation, None)?;
            let row = response_rows(ep, &domain.defs)
                .into_iter()
                .find(|r| r.status == status)
                .ok_or_else(|| {
                    crate::error::GddyError::not_found(format!(
                        "response '{status}' not found on operation '{}'",
                        ep.operation_id
                    ))
                    .with_fix(format!(
                        "Run: gddy api response list --operation {}",
                        ep.operation_id
                    ))
                    .into_cli_error()
                })?;
            let next_actions = row
                .schema_id
                .clone()
                .map(|id| {
                    next_action("api schema get <id>", "See the full response schema")
                        .with_param("id", required_value(id))
                })
                .into_iter()
                .collect();
            Ok(CommandResult::new(to_json(row)?).with_next_actions(next_actions))
        },
    )
}

#[derive(Debug, Clone, clap::Args)]
struct SchemaGetArgs {
    /// Schema id — a component name, or a dotted path from a truncated preview.
    #[arg(value_name = "ID")]
    id: String,
}

fn schema_get_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed::<SchemaGetArgs, _, _, _>(
        CommandSpec::from_args::<SchemaGetArgs>("get", "Show a schema's full nested tree")
            .with_long(
                "Shows the complete nested structure of a schema — every property at every \
                 level, with $ref's resolved inline. This is the terminal drill-down target \
                 for a truncated schema preview shown elsewhere: it never truncates itself. \
                 Accepts either a shared component name (e.g. Business) or the dotted id \
                 shown alongside a truncated preview (e.g. getUser.responses.200.schema).",
            )
            .with_system("api")
            .with_tier(Tier::Read)
            .no_auth(true),
        |_cred, args: SchemaGetArgs| async move {
            let id = args.id.as_str();
            let catalog = catalog();

            // A bare component name (no schema-id keyword segment) is looked
            // up directly against every domain's `$defs` — it isn't scoped
            // to one operation the way an inline dotted id is.
            if let Some((domain_defs, schema)) = catalog
                .iter()
                .find_map(|d| d.defs.get(id).map(|schema| (&d.defs, schema)))
            {
                let tree = build_schema_tree(id, schema, domain_defs);
                return Ok(CommandResult::new(to_json(tree)?));
            }

            let (operation_id, location) = parse_schema_id(id).ok_or_else(|| {
                crate::error::GddyError::not_found(format!("no schema found for id '{id}'"))
                    .with_fix("Run: gddy api operation get <operationId> to find schema ids")
                    .into_cli_error()
            })?;

            let (domain, ep) = resolve_operation(catalog, &operation_id, None)?;

            let raw_schema = match &location {
                SchemaLocation::RequestBody => {
                    ep.request_body.as_ref().and_then(|b| b.get("schema"))
                }
                SchemaLocation::Parameter(name) => {
                    find_parameter_schema(ep, name).map(|(schema, _)| schema)
                }
                SchemaLocation::Response(status) => ep
                    .responses
                    .get(status.as_str())
                    .and_then(|r| r.get("schema")),
            };
            let raw_schema = raw_schema.ok_or_else(|| {
                crate::error::GddyError::not_found(format!("no schema found for id '{id}'"))
                    .into_cli_error()
            })?;

            let tree = build_schema_tree(id, raw_schema, &domain.defs);
            Ok(CommandResult::new(to_json(tree)?))
        },
    )
}

#[cfg(test)]
mod tests {
    use cli_engine::{Cli, CliConfig};

    use super::{catalog, find_endpoint, merge_required_scopes};

    fn v(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn merge_flags_only_when_no_endpoint_scopes() {
        assert_eq!(merge_required_scopes(v(&["a", "b"]), &[]), v(&["a", "b"]));
    }

    #[test]
    fn merge_endpoint_only_when_no_flags() {
        assert_eq!(
            merge_required_scopes(v(&[]), &v(&["x", "y"])),
            v(&["x", "y"])
        );
    }

    #[test]
    fn merge_unions_and_dedupes_flags_first() {
        assert_eq!(
            merge_required_scopes(v(&["a", "b"]), &v(&["b", "c"])),
            v(&["a", "b", "c"])
        );
    }

    #[test]
    fn merge_dedupes_repeated_flag_values() {
        assert_eq!(
            merge_required_scopes(v(&["a", "a", "b"]), &v(&["b"])),
            v(&["a", "b"])
        );
    }

    /// The commerce endpoint's catalog scope is included alongside an explicit
    /// scope. `call_command` sends this exact list to
    /// `credential_with_scopes` before it builds the HTTP request, so the PKCE
    /// provider can perform OAuth step-up before an API 403.
    #[test]
    fn commerce_catalog_scope_is_merged_with_an_explicit_scope() {
        let (_, endpoint) = find_endpoint(catalog(), "/v1/commerce/stores/{storeId}/orders")
            .expect("orders endpoint exists in the embedded catalog");
        assert_eq!(
            merge_required_scopes(v(&["commerce.order:write"]), &endpoint.scopes),
            v(&["commerce.order:write", "commerce.order:read"]),
        );
    }

    /// `catalog()` sorts once so every listing (`api domain list`, `api
    /// search`, `api operation get`) sees the same stable, alphabetical order.
    #[test]
    fn catalog_domains_are_sorted_alphabetically() {
        let names: Vec<&str> = catalog().iter().map(|d| d.name.as_str()).collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
    }

    /// Mirrors `call_command`'s domain-aware base-URL resolution: a matched
    /// catalog endpoint resolves through its own domain's host (not the
    /// generic gateway), with per-environment convention substitution.
    #[test]
    fn matched_endpoint_resolves_its_own_domain_base_url() {
        let (domain, _) = find_endpoint(catalog(), "listFulfillments")
            .expect("listFulfillments exists in the embedded catalog");
        assert_eq!(domain.name, "fulfillments");
        let base_url =
            crate::environments::resolve_catalog_base_url(&domain.name, &domain.base_url, "ote");
        assert_eq!(
            base_url,
            "https://fulfillment.api.commerce.ote-godaddy.com/v1/commerce"
        );
    }

    /// A real request path with path params substituted (as `api call`
    /// receives — no user passes a literal `{storeId}`) must still match its
    /// templated catalog path, or every parameterized endpoint would fall
    /// back to the wrong (generic gateway) base URL and lose scope lookup.
    #[test]
    fn concrete_path_matches_its_templated_catalog_path() {
        let (domain, endpoint) = find_endpoint(catalog(), "/stores/abc123/fulfillments")
            .expect("concrete path should match the templated catalog path");
        assert_eq!(domain.name, "fulfillments");
        assert_eq!(endpoint.operation_id, "listFulfillments");
    }

    /// An endpoint the catalog doesn't recognize has no domain to resolve a
    /// base URL against — `call_command` falls back to the generic gateway
    /// host in this case.
    #[test]
    fn unmatched_endpoint_has_no_domain_to_resolve_against() {
        assert!(find_endpoint(catalog(), "not-a-real-operation-id").is_none());
    }

    #[test]
    fn split_header_trims_and_splits_on_first_colon() {
        assert_eq!(
            super::split_header("x-store-id: abc-123"),
            Some(("x-store-id", "abc-123"))
        );
        // Value may contain colons (e.g. a URL) — only the first colon splits.
        assert_eq!(
            super::split_header("Location: https://x/y"),
            Some(("Location", "https://x/y"))
        );
        assert_eq!(super::split_header("no-colon"), None);
    }

    #[test]
    fn parse_response_body_json_text_and_empty() {
        use serde_json::json;
        assert_eq!(super::parse_response_body(b"{\"a\":1}"), json!({"a":1}));
        // Non-JSON is preserved as text, not dropped to null.
        assert_eq!(
            super::parse_response_body(b"plain text"),
            json!("plain text")
        );
        // Empty body is null.
        assert_eq!(super::parse_response_body(b""), serde_json::Value::Null);
    }

    #[test]
    fn graphql_errors_detects_nonempty_array_only() {
        use serde_json::json;
        assert!(
            super::graphql_errors(&json!({"data": null, "errors": [{"message": "x"}]})).is_some()
        );
        assert!(super::graphql_errors(&json!({"data": {}, "errors": []})).is_none());
        assert!(super::graphql_errors(&json!({"data": {}})).is_none());
    }

    #[test]
    fn is_mutating_method_treats_only_get_and_head_as_safe() {
        assert!(!super::is_mutating_method("GET"));
        assert!(!super::is_mutating_method("get"));
        assert!(!super::is_mutating_method("HEAD"));
        assert!(super::is_mutating_method("POST"));
        assert!(super::is_mutating_method("PUT"));
        assert!(super::is_mutating_method("PATCH"));
        assert!(super::is_mutating_method("DELETE"));
    }

    /// `call` opted into handler-driven `--dry-run` so a GET/HEAD can execute
    /// for real under `--dry-run` — but it still requires `Required` auth
    /// (no `.no_auth()`), so it must stay fail-closed regardless of method.
    #[tokio::test]
    async fn call_dry_run_get_still_requires_auth() {
        const AUTH_FAILURE_EXIT: i32 = 2;
        let cli = Cli::new(
            CliConfig::new("gddy", "GoDaddy developer CLI", "gddy")
                .with_default_auth_provider("godaddy")
                .with_module(super::module()),
        );
        let output = cli
            .run([
                "gddy",
                "api",
                "call",
                "/v1/example",
                "--method",
                "GET",
                "--dry-run",
                "--output",
                "json",
            ])
            .await;
        assert_eq!(
            output.exit_code, AUTH_FAILURE_EXIT,
            "a GET falls through to the real request even under --dry-run, so it still needs \
             auth, got: {}",
            output.rendered
        );
    }

    /// `call` is `auth_optional()` specifically so the dry-run-for-mutating-
    /// methods branch never triggers credential resolution (previously it did,
    /// via the engine's `Required`-auth eager resolution before the handler
    /// even ran) — a POST preview must succeed with no auth provider at all.
    #[tokio::test]
    async fn call_dry_run_mutating_method_needs_no_auth() {
        let cli = Cli::new(
            CliConfig::new("gddy", "GoDaddy developer CLI", "gddy").with_module(super::module()),
        );
        let output = cli
            .run([
                "gddy",
                "api",
                "call",
                "/v1/example",
                "--method",
                "POST",
                "--dry-run",
                "--output",
                "json",
            ])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let rendered: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json");
        assert_eq!(rendered["data"]["action"], "dry-run: would execute");
    }

    /// A malformed method must still be rejected under `--dry-run`, matching
    /// what the real path's `method.parse()` would do — a garbage method
    /// must not previously have returned a fake "would execute" success.
    #[tokio::test]
    async fn call_dry_run_rejects_a_malformed_method() {
        let cli = Cli::new(
            CliConfig::new("gddy", "GoDaddy developer CLI", "gddy").with_module(super::module()),
        );
        let output = cli
            .run([
                "gddy",
                "api",
                "call",
                "/v1/example",
                "--method",
                "POST ",
                "--dry-run",
                "--output",
                "json",
            ])
            .await;
        assert_ne!(output.exit_code, 0, "{}", output.rendered);
        assert!(
            output.rendered.contains("invalid HTTP method"),
            "{}",
            output.rendered
        );
    }

    /// A bare operationId (no leading '/') must be rejected rather than
    /// silently built into an invalid request URL — `find_endpoint` can match
    /// it for scope lookup, but the URL is always built from the literal
    /// `endpoint` string, not the matched catalog path.
    #[tokio::test]
    async fn call_rejects_an_endpoint_without_a_leading_slash() {
        let cli = Cli::new(
            CliConfig::new("gddy", "GoDaddy developer CLI", "gddy").with_module(super::module()),
        );
        let output = cli
            .run([
                "gddy",
                "api",
                "call",
                "listFulfillments",
                "--dry-run",
                "--output",
                "json",
            ])
            .await;
        assert_ne!(output.exit_code, 0, "{}", output.rendered);
        assert!(
            output.rendered.contains("must be a URL path starting with"),
            "{}",
            output.rendered
        );
    }

    // -----------------------------------------------------------------------
    // Schema summarization
    // -----------------------------------------------------------------------

    use serde_json::json;

    use super::{schema_type_label, search_endpoints, summarize_graphql_schema, summarize_schema};

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

    // -----------------------------------------------------------------------
    // Full schema trees and their ids
    // -----------------------------------------------------------------------

    use super::{
        SCHEMA_ID_KEYWORDS, SchemaLocation, build_schema_tree, compute_schema_id, describe_schema,
        parse_schema_id, scalar_schema_type, schema_base_type_label,
    };

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

    // -----------------------------------------------------------------------
    // GraphQL summarization
    // -----------------------------------------------------------------------

    fn make_graphql_operations(count: usize) -> Vec<super::GraphqlOperation> {
        (0..count)
            .map(|i| super::GraphqlOperation {
                name: format!("op{i}"),
                kind: if i % 2 == 0 { "query" } else { "mutation" }.to_owned(),
                return_type: "String".to_owned(),
                deprecated: false,
                deprecation_reason: None,
                args: vec![],
            })
            .collect()
    }

    #[test]
    fn summarize_graphql_schema_includes_every_operation_and_splits_query_mutation_counts() {
        let schema = super::GraphqlSchema {
            schema_ref: "./schema.graphql".to_owned(),
            operation_count: 25,
            operations: make_graphql_operations(25),
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
        let schema = super::GraphqlSchema {
            schema_ref: "./schema.graphql".to_owned(),
            operation_count: 1,
            operations: vec![super::GraphqlOperation {
                name: "widgets".to_owned(),
                kind: "query".to_owned(),
                return_type: "[Widget]".to_owned(),
                deprecated: true,
                deprecation_reason: Some("use widgetsV2".to_owned()),
                args: vec![super::GraphqlArgument {
                    name: "id".to_owned(),
                    arg_type: "String!".to_owned(),
                    required: true,
                    default_value: None,
                }],
            }],
        };
        let summary = summarize_graphql_schema(&schema);
        assert_eq!(summary["operations"][0]["name"], json!("widgets"));
        assert_eq!(summary["operations"][0]["deprecated"], json!(true));
        assert_eq!(
            summary["operations"][0]["args"][0]["type"],
            json!("String!")
        );
    }

    // -----------------------------------------------------------------------
    // search_endpoints — GraphQL operation names are searchable
    // -----------------------------------------------------------------------

    #[test]
    fn search_endpoints_matches_a_graphql_operation_name() {
        let hits = search_endpoints(catalog(), "SKUGroup");
        assert!(
            hits.iter()
                .any(|(_, ep)| ep.operation_id == "postCatalogGraphql"),
            "expected a GraphQL operation name to surface its parent endpoint in search results"
        );
    }

    // -----------------------------------------------------------------------
    // api operation get — exact/method/fuzzy resolution, schema + GraphQL summary
    // -----------------------------------------------------------------------

    fn operation_cli() -> Cli {
        // `resolve_catalog_base_url` needs `ctx.middleware.env` to actually
        // resolve to `DEFAULT_ENV` ("prod") rather than an empty default, so
        // wire environments the same way `main.rs` does for the real CLI.
        Cli::new(
            CliConfig::new("gddy", "GoDaddy developer CLI", "gddy")
                .with_module(super::module())
                .with_environments(std::sync::Arc::clone(crate::environments::instance())),
        )
    }

    #[tokio::test]
    async fn operation_get_exact_operation_id_match() {
        // Pin `--env` explicitly: `fullPath` depends on env-resolved base
        // URL, and the default env is read from ambient local config
        // (`gdenv`), so a bare run isn't hermetic across machines/CI.
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "operation",
                "get",
                "commerce.location.verify-address",
                "--env",
                "prod",
                "--output",
                "json",
            ])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let rendered: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json");
        assert_eq!(
            rendered["data"]["operationId"],
            json!("commerce.location.verify-address")
        );
        assert_eq!(
            rendered["data"]["fullPath"],
            json!("/v1/commerce/location/address-verifications")
        );
    }

    /// `fullPath` must stay a hostless path in every environment, not just
    /// prod — `resolve_catalog_base_url` rewrites the host for non-prod envs
    /// (e.g. `api.ote-godaddy.com`), so stripping only the literal prod host
    /// would silently leave the scheme+host in place here.
    #[tokio::test]
    async fn operation_get_full_path_is_hostless_in_a_non_prod_env() {
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "operation",
                "get",
                "commerce.location.verify-address",
                "--env",
                "ote",
                "--output",
                "json",
            ])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let rendered: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json");
        assert_eq!(
            rendered["data"]["baseUrl"],
            json!("https://api.ote-godaddy.com/v1/commerce")
        );
        assert_eq!(
            rendered["data"]["fullPath"],
            json!("/v1/commerce/location/address-verifications")
        );
    }

    #[tokio::test]
    async fn operation_get_exact_match_narrowed_by_method_flag() {
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "operation",
                "get",
                "/location/addresses",
                "--method",
                "GET",
                "--output",
                "json",
            ])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let rendered: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json");
        assert_eq!(
            rendered["data"]["operationId"],
            json!("commerce.location.search-addresses")
        );
    }

    /// Matches the original CLI's documented quirk: `--method` only narrows
    /// the exact-match step. A mismatched method falls through to fuzzy
    /// search, which ignores the method filter entirely — so this still
    /// resolves to the (wrong-method) endpoint rather than erroring.
    #[tokio::test]
    async fn operation_get_method_filter_is_ignored_during_fuzzy_fallback() {
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "operation",
                "get",
                "/location/addresses",
                "--method",
                "POST",
                "--output",
                "json",
            ])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let rendered: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json");
        assert_eq!(
            rendered["data"]["operationId"],
            json!("commerce.location.search-addresses")
        );
        assert_eq!(rendered["data"]["method"], json!("GET"));
    }

    #[tokio::test]
    async fn operation_get_single_fuzzy_match_resolves_transparently() {
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "operation",
                "get",
                "verify-address",
                "--output",
                "json",
            ])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let rendered: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json");
        assert_eq!(
            rendered["data"]["operationId"],
            json!("commerce.location.verify-address")
        );
    }

    /// A fuzzy query matching several unrelated endpoints is a hard error
    /// (see `resolve_operation`/`ambiguous_operation_error`), not a
    /// success-shaped `{message, matches}` response — cli-engine's error
    /// envelope has no structured `next_actions` hook, so every candidate
    /// is instead a runnable command line inside the error's `fix` string.
    #[tokio::test]
    async fn operation_get_multiple_fuzzy_matches_is_an_ambiguous_error() {
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "operation",
                "get",
                "/location",
                "--output",
                "json",
            ])
            .await;
        assert_ne!(output.exit_code, 0, "{}", output.rendered);
        let rendered: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json");
        assert_eq!(rendered["error"]["code"], json!("AMBIGUOUS_MATCH"));
        assert!(
            rendered["error"]["message"]
                .as_str()
                .unwrap_or_default()
                .contains("matches 2 operations"),
            "{}",
            output.rendered
        );
        let fix = rendered["fix"].as_str().expect("fix present");
        assert!(fix.contains("/location/address-verifications"), "{fix}");
        assert!(fix.contains("/location/addresses"), "{fix}");
        assert!(fix.contains("gddy api operation get"), "{fix}");
    }

    /// Same ambiguity, but through human output — covers that rendering
    /// path directly, since every other test here only exercises
    /// `--output json`. Human error rendering is `Error: {message}` /
    /// `Fix: {fix}` (`cli_engine::output::human::render_human_with_view`).
    #[tokio::test]
    async fn operation_get_multiple_fuzzy_matches_human_output_shows_message_and_candidates() {
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "operation",
                "get",
                "/location",
                "--output",
                "human",
            ])
            .await;
        assert_ne!(output.exit_code, 0, "{}", output.rendered);
        assert!(
            output.rendered.contains("'/location' matches 2 operations"),
            "{}",
            output.rendered
        );
        assert!(
            output.rendered.contains("/location/address-verifications"),
            "{}",
            output.rendered
        );
        assert!(
            output.rendered.contains("/location/addresses"),
            "{}",
            output.rendered
        );
    }

    /// Many catalog paths are shared by several endpoints that only differ
    /// by method (here: `GET /businesses` and `POST /businesses`) — without
    /// `--method`, exact-match resolution must treat that the same as an
    /// ambiguous fuzzy match, not silently pick whichever one it saw first.
    #[tokio::test]
    async fn operation_get_exact_path_shared_by_multiple_methods_is_ambiguous() {
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "operation",
                "get",
                "/businesses",
                "--output",
                "json",
            ])
            .await;
        assert_ne!(output.exit_code, 0, "{}", output.rendered);
        let rendered: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json");
        assert_eq!(rendered["error"]["code"], json!("AMBIGUOUS_MATCH"));
        // Candidates sharing one path are only distinguishable by method, so
        // the fix must spell out both.
        let fix = rendered["fix"].as_str().expect("fix present");
        assert!(fix.contains("--method GET"), "{fix}");
        assert!(fix.contains("--method POST"), "{fix}");
    }

    /// The same ambiguity, resolved by adding `--method`.
    #[tokio::test]
    async fn operation_get_exact_path_shared_by_multiple_methods_resolves_with_method_flag() {
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "operation",
                "get",
                "/businesses",
                "--method",
                "POST",
                "--output",
                "json",
            ])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let rendered: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json");
        assert_eq!(rendered["data"]["operationId"], json!("createBusiness"));
    }

    /// A concrete path with a real ID substituted for a `{param}` segment
    /// must resolve via template matching, same as `api call` already does
    /// via `find_endpoint`/`path_matches_template`.
    #[tokio::test]
    async fn operation_get_concrete_path_resolves_via_template_matching() {
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "operation",
                "get",
                "/businesses/abc123",
                "--method",
                "GET",
                "--output",
                "json",
            ])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let rendered: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json");
        assert_eq!(rendered["data"]["operationId"], json!("getBusinessById"));
    }

    #[tokio::test]
    async fn operation_get_zero_matches_is_an_error() {
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "operation",
                "get",
                "totally-fake-endpoint-xyz",
                "--output",
                "json",
            ])
            .await;
        assert_ne!(output.exit_code, 0, "{}", output.rendered);
        assert!(
            output.rendered.contains("no operation found matching"),
            "{}",
            output.rendered
        );
    }

    /// Regression: `.with_view(...)` covered `parameters.items` but had no
    /// `responses.items` column at all, so `responses` — present in the
    /// JSON the whole time — silently never rendered in `--output human`,
    /// with no error or missing-column notice to say so.
    #[tokio::test]
    async fn operation_get_human_output_shows_a_responses_table() {
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "operation",
                "get",
                "createShipment",
                "--output",
                "human",
            ])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        assert!(
            output.rendered.contains("Responses:"),
            "{}",
            output.rendered
        );
        assert!(output.rendered.contains("STATUS"), "{}", output.rendered);
        assert!(output.rendered.contains("200"), "{}", output.rendered);
    }

    /// Most catalog request bodies are a bare `$ref` with no inline
    /// `properties` (63/82 in the embedded catalog) — `createBusiness`'s is
    /// `{"$ref": "#/$defs/Business"}`. Without $ref resolution this
    /// summarizes to `null`; with it, the real fields show up. The request
    /// body is folded into `parameters` as the synthetic `body` row.
    #[tokio::test]
    async fn operation_get_resolves_a_pure_ref_request_body_against_defs() {
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "operation",
                "get",
                "createBusiness",
                "--output",
                "json",
            ])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let rendered: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json");
        let parameters = rendered["data"]["parameters"]["items"]
            .as_array()
            .expect("parameters items array");
        let body = parameters
            .iter()
            .find(|p| p["name"] == "body")
            .expect("synthetic body row");
        let schema = body["schema"]["items"]
            .as_array()
            .expect("body.schema resolves to a property list, not null");
        let active_since = schema
            .iter()
            .find(|p| p["name"] == "activeSince")
            .expect("activeSince property");
        assert_eq!(active_since["type"], json!("string(date-time)"));
        assert_eq!(body["schemaId"], json!("Business"));
    }

    /// `getChannels` has 6 parameters — under the preview cap, so its
    /// `operation get` output isn't truncated and shouldn't link to the
    /// standalone `parameter list`.
    #[tokio::test]
    async fn operation_get_does_not_link_to_parameter_list_when_untruncated() {
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "operation",
                "get",
                "getChannels",
                "--output",
                "json",
            ])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let rendered: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json");
        assert_eq!(
            rendered["data"]["parameters"]["pagination"]["total"],
            json!(6)
        );
        assert!(
            !rendered["data"]["parameters"]["pagination"]["has_more"]
                .as_bool()
                .unwrap_or(true)
        );
        let next_actions = rendered["next_actions"].as_array().expect("next_actions");
        assert!(
            next_actions
                .iter()
                .any(|a| a["command"].as_str().unwrap_or("").contains("api call")),
            "{}",
            output.rendered
        );
        assert!(
            !next_actions.iter().any(|a| a["command"]
                .as_str()
                .unwrap_or("")
                .contains("parameter list")),
            "an untruncated preview should not link to the standalone list: {}",
            output.rendered
        );
    }

    /// `createShipment`'s synthetic `body` parameter has a `schemaId`
    /// (`Shipment`, a `$ref`) — a caller needs to be told `api schema get`
    /// exists at all to make use of it, or the id in the table is a dead
    /// end. One generic hint covers every parameter/response with a
    /// `schemaId`, rather than a same-command line repeated per row.
    #[tokio::test]
    async fn operation_get_links_to_schema_get_when_any_row_has_a_schema_id() {
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "operation",
                "get",
                "createShipment",
                "--output",
                "json",
            ])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let rendered: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json");
        let next_actions = rendered["next_actions"].as_array().expect("next_actions");
        let hint = next_actions
            .iter()
            .find(|a| a["command"] == json!("gddy api schema get <id>"))
            .expect("generic schema-get hint");
        assert!(hint["params"]["id"]["required"].as_bool().unwrap_or(false));
        assert!(hint["params"]["id"]["value"].is_null());
    }

    /// `deleteDNSRecord` has no schema anywhere (every parameter is a bare
    /// scalar, and its one response is a bare `204` with no body) — the
    /// schema-get hint must not appear when there's nothing for it to point
    /// at.
    #[tokio::test]
    async fn operation_get_does_not_link_to_schema_get_when_no_row_has_a_schema_id() {
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "operation",
                "get",
                "deleteDNSRecord",
                "--output",
                "json",
            ])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let rendered: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json");
        let next_actions = rendered["next_actions"].as_array().expect("next_actions");
        assert!(
            !next_actions
                .iter()
                .any(|a| a["command"] == json!("gddy api schema get <id>")),
            "{}",
            output.rendered
        );
    }

    /// `get_transaction_disputes` has 26 parameters — over the preview cap
    /// — so its `operation get` output must be truncated and link to the
    /// standalone `parameter list` for the rest, per the "list truncated →
    /// standalone list command" convention.
    #[tokio::test]
    async fn operation_get_links_to_parameter_list_when_truncated() {
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "operation",
                "get",
                "get_transaction_disputes",
                "--output",
                "json",
            ])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let rendered: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json");
        // `Summary`'s only field beyond `items` is `pagination` (a
        // `PaginationMeta`) — the same shape cli-engine's own top-level
        // pagination uses, so cli-engine 0.8.2 (DEVEX-985) can read it as a
        // sibling of a nested column's array and render the same "(N of M
        // rows, offset O, limit L)" footer a top-level paginated array gets.
        assert_eq!(
            rendered["data"]["parameters"]["pagination"]["total"],
            json!(26)
        );
        assert_eq!(
            rendered["data"]["parameters"]["pagination"]["count"],
            json!(20)
        );
        assert_eq!(
            rendered["data"]["parameters"]["pagination"]["limit"],
            json!(20)
        );
        assert_eq!(
            rendered["data"]["parameters"]["pagination"]["has_more"],
            json!(true)
        );
        let next_actions = rendered["next_actions"].as_array().expect("next_actions");
        let link = next_actions
            .iter()
            .find(|a| {
                a["command"]
                    .as_str()
                    .unwrap_or("")
                    .contains("parameter list")
            })
            .expect("next_action linking to the standalone parameter list");
        assert_eq!(
            link["params"]["operation"]["value"],
            json!("get_transaction_disputes")
        );
    }

    /// Same fixture as `operation_get_links_to_parameter_list_when_truncated`,
    /// through human output: DEVEX-985 (cli-engine 0.8.2) lets a nested
    /// table render a truncation footer at all, so this is the first time
    /// `--output human` can show "not all rows are here" for `operation
    /// get`'s embedded parameter preview, rather than a bare `(20 rows)`
    /// with no hint that 6 more exist.
    #[tokio::test]
    async fn operation_get_human_output_shows_truncation_footer_for_parameters() {
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "operation",
                "get",
                "get_transaction_disputes",
                "--output",
                "human",
            ])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        assert!(
            output
                .rendered
                .contains("(20 of 26 rows, offset 0, limit 20)"),
            "{}",
            output.rendered
        );
    }

    #[tokio::test]
    async fn operation_get_graphql_endpoint_gets_summary() {
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "operation",
                "get",
                "postCatalogGraphql",
                "--output",
                "json",
            ])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let rendered: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json");
        let data = &rendered["data"];
        assert_eq!(data["graphql"]["operationCount"], json!(149));
        assert_eq!(
            data["graphql"]["operations"]
                .as_array()
                .expect("operations array")
                .len(),
            149
        );
    }

    // -----------------------------------------------------------------------
    // api parameter / api response / api schema
    // -----------------------------------------------------------------------

    /// `commerce.location.verify-address` has no real parameters, only a
    /// request body — so its parameter list is exactly the synthetic `body`
    /// row, folded in per DEVEX-967.
    #[tokio::test]
    async fn parameter_list_folds_in_the_synthetic_body_row() {
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "parameter",
                "list",
                "--operation",
                "commerce.location.verify-address",
                "--output",
                "json",
            ])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let rendered: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json");
        let items = rendered["data"].as_array().expect("data array");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["name"], json!("body"));
        assert_eq!(items[0]["in"], json!("body"));
        assert_eq!(items[0]["required"], json!(true));
    }

    /// Regression: without a declared view, cli-engine's no-view human
    /// renderer falls back to alphabetical column order for a bare array
    /// (`dynamic_columns` in cli-engine's `output/human.rs`), which
    /// scrambled this relative to `operation get`'s own `parameters.items`
    /// nested view. `parameter_list_command` declares a matching
    /// `.with_view(...)` specifically to avoid that.
    #[tokio::test]
    async fn parameter_list_human_output_uses_the_declared_column_order() {
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "parameter",
                "list",
                "--operation",
                "createShipment",
                "--output",
                "human",
            ])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let header_line = output
            .rendered
            .lines()
            .next()
            .expect("at least a header line")
            .trim_end();
        assert_eq!(
            header_line, "NAME     IN    REQUIRED  TYPE    SCHEMA ID  DESCRIPTION",
            "{}",
            output.rendered
        );
    }

    /// Same regression, for `response_list_command`.
    #[tokio::test]
    async fn response_list_human_output_uses_the_declared_column_order() {
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "response",
                "list",
                "--operation",
                "createShipment",
                "--output",
                "human",
            ])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let header_line = output
            .rendered
            .lines()
            .next()
            .expect("at least a header line")
            .trim_end();
        assert_eq!(
            header_line, "STATUS  TYPE    SCHEMA ID",
            "{}",
            output.rendered
        );
    }

    /// `--limit`/`--offset` here come from `CommandSpec::with_pagination`
    /// (cli-engine 0.7.0's builder), and the resulting page + pagination
    /// metadata + "next page" hint are entirely cli-engine 0.8's doing —
    /// this command returns the full unsliced list and does none of that
    /// itself. Exercises the whole pipeline end-to-end, not a unit in
    /// isolation.
    #[tokio::test]
    async fn parameter_list_honors_engine_provided_limit_and_offset() {
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "parameter",
                "list",
                "--operation",
                "get_transaction_disputes",
                "--limit",
                "5",
                "--offset",
                "20",
                "--output",
                "json",
            ])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let rendered: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json");
        assert_eq!(rendered["data"].as_array().expect("data array").len(), 5);
        let pagination = &rendered["pagination"];
        assert_eq!(pagination["total"], json!(26));
        assert_eq!(pagination["offset"], json!(20));
        assert_eq!(pagination["limit"], json!(5));
        assert_eq!(pagination["count"], json!(5));
        // 20 + 5 = 25 < 26, so one more remains.
        assert_eq!(pagination["has_more"], json!(true));
        let next_actions = rendered["next_actions"].as_array().expect("next_actions");
        assert!(
            next_actions
                .iter()
                .any(|a| a["command"].as_str().unwrap_or("").contains("--offset 25")),
            "expected an auto-generated next-page hint: {}",
            output.rendered
        );
    }

    /// `--limit 0` means "all" per cli-engine's own generated help text for
    /// `with_pagination` — must show every item, not zero. With both
    /// `--limit`/`--offset` at their "disabled" value (0), cli-engine 0.8's
    /// pipeline skips pagination entirely rather than reporting a page of
    /// everything, so there's no `pagination` envelope field at all here.
    #[tokio::test]
    async fn parameter_list_limit_zero_means_all() {
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "parameter",
                "list",
                "--operation",
                "get_transaction_disputes",
                "--limit",
                "0",
                "--output",
                "json",
            ])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let rendered: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json");
        assert_eq!(rendered["data"].as_array().expect("data array").len(), 26);
        assert!(rendered.get("pagination").is_none(), "{}", output.rendered);
    }

    /// `max_limit` (`OPERATION_CHILD_LIST_MAX_LIMIT`) rejects an
    /// out-of-range `--limit` at parse time, before the handler even runs.
    #[tokio::test]
    async fn parameter_list_limit_above_max_is_rejected() {
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "parameter",
                "list",
                "--operation",
                "get_transaction_disputes",
                "--limit",
                "999",
                "--output",
                "json",
            ])
            .await;
        assert_ne!(output.exit_code, 0, "{}", output.rendered);
    }

    #[tokio::test]
    async fn parameter_get_resolves_the_synthetic_body_name() {
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "parameter",
                "get",
                "body",
                "--operation",
                "commerce.location.verify-address",
                "--output",
                "json",
            ])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let rendered: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json");
        assert_eq!(rendered["data"]["name"], json!("body"));
        assert_eq!(rendered["data"]["in"], json!("body"));
        assert!(rendered["data"]["schema"]["items"].is_array());
        assert_eq!(rendered["data"]["type"], json!("object"));
        let schema_id = rendered["data"]["schemaId"]
            .as_str()
            .expect("schemaId present whenever a schema exists");
        let next_actions = rendered["next_actions"].as_array().expect("next_actions");
        let link = next_actions
            .iter()
            .find(|a| a["command"].as_str().unwrap_or("").contains("schema get"))
            .expect("schema next_action attached even though the preview isn't truncated");
        assert_eq!(link["params"]["id"]["value"], json!(schema_id));
    }

    /// `storeId` on `queryCarriers` has no `schema` at all — the id and its
    /// "see full schema" next_action should both be absent, not present with
    /// an empty/garbage value.
    #[tokio::test]
    async fn parameter_get_without_a_schema_has_no_schema_id_or_next_action() {
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "parameter",
                "get",
                "storeId",
                "--operation",
                "queryCarriers",
                "--output",
                "json",
            ])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let rendered: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json");
        assert!(rendered["data"]["schemaId"].is_null());
        assert!(
            rendered["next_actions"]
                .as_array()
                .map(|a| a.is_empty())
                .unwrap_or(true),
            "{}",
            output.rendered
        );
    }

    /// `pageSize` on `queryCarriers` has a bare inline scalar schema
    /// (`{"type": "integer", "format": "int64", ...}`, no `properties`, no
    /// `$ref`) — there's nothing to drill into, so it should surface `type`
    /// directly rather than a `schemaId`/next_action pointing at a
    /// `schema get` that would just echo the same type back.
    #[tokio::test]
    async fn parameter_get_scalar_schema_shows_type_instead_of_schema_id() {
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "parameter",
                "get",
                "pageSize",
                "--operation",
                "queryCarriers",
                "--output",
                "json",
            ])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let rendered: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json");
        assert!(rendered["data"]["schemaId"].is_null());
        assert_eq!(rendered["data"]["type"], json!("integer(int64)"));
        assert!(
            rendered["next_actions"]
                .as_array()
                .map(|a| a.is_empty())
                .unwrap_or(true),
            "{}",
            output.rendered
        );
    }

    #[tokio::test]
    async fn parameter_get_unknown_name_is_a_not_found_error() {
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "parameter",
                "get",
                "does-not-exist",
                "--operation",
                "commerce.location.verify-address",
                "--output",
                "json",
            ])
            .await;
        assert_ne!(output.exit_code, 0, "{}", output.rendered);
        assert!(output.rendered.contains("not found"), "{}", output.rendered);
    }

    /// `getChannels`'s `registeredStores.storeId` parameter has a literal
    /// dot in its name — a real edge case for the schema-id grammar (see
    /// `schema_id_round_trip_holds_across_the_real_catalog`), exercised here
    /// end-to-end through the actual command.
    #[tokio::test]
    async fn parameter_get_handles_a_dotted_parameter_name() {
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "parameter",
                "get",
                "registeredStores.storeId",
                "--operation",
                "getChannels",
                "--output",
                "json",
            ])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let rendered: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json");
        assert_eq!(rendered["data"]["name"], json!("registeredStores.storeId"));
    }

    #[tokio::test]
    async fn response_list_returns_every_status_sorted() {
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "response",
                "list",
                "--operation",
                "patchBusiness",
                "--output",
                "json",
            ])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let rendered: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json");
        let items = rendered["data"].as_array().expect("data array");
        let statuses: Vec<&str> = items
            .iter()
            .map(|r| r["status"].as_str().expect("status"))
            .collect();
        assert_eq!(statuses, vec!["200", "404", "default"]);
    }

    #[tokio::test]
    async fn response_get_returns_one_status_with_its_schema_preview() {
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "response",
                "get",
                "200",
                "--operation",
                "commerce.location.verify-address",
                "--output",
                "json",
            ])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let rendered: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json");
        assert_eq!(rendered["data"]["status"], json!("200"));
        let schema_items = rendered["data"]["schema"]["items"]
            .as_array()
            .expect("schema items array");
        let names: Vec<&str> = schema_items
            .iter()
            .map(|p| p["name"].as_str().expect("name"))
            .collect();
        assert!(names.contains(&"status"));
        assert!(names.contains(&"data"));
        assert_eq!(rendered["data"]["type"], json!("object"));
        let schema_id = rendered["data"]["schemaId"]
            .as_str()
            .expect("schemaId present whenever a schema exists");
        let next_actions = rendered["next_actions"].as_array().expect("next_actions");
        let link = next_actions
            .iter()
            .find(|a| a["command"].as_str().unwrap_or("").contains("schema get"))
            .expect("schema next_action attached even though the preview isn't truncated");
        assert_eq!(link["params"]["id"]["value"], json!(schema_id));
    }

    #[tokio::test]
    async fn response_get_unknown_status_is_a_not_found_error() {
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "response",
                "get",
                "599",
                "--operation",
                "commerce.location.verify-address",
                "--output",
                "json",
            ])
            .await;
        assert_ne!(output.exit_code, 0, "{}", output.rendered);
        assert!(output.rendered.contains("not found"), "{}", output.rendered);
    }

    /// A `$ref` schema's id is its bare `$defs` key — no operation scoping
    /// needed to look it up.
    #[tokio::test]
    async fn schema_get_resolves_a_bare_defs_name() {
        let output = operation_cli()
            .run([
                "gddy", "api", "schema", "get", "address", "--output", "json",
            ])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let rendered: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json");
        assert_eq!(rendered["data"]["type"], json!("object"));
        let properties = rendered["data"]["properties"]
            .as_array()
            .expect("properties array");
        assert!(properties.iter().any(|p| p["name"] == "addressDetails"));
    }

    /// An inline response schema's id is a dotted path rooted at the
    /// operationId — which, for this operation, itself contains dots, so
    /// this also exercises the schema-id parser's keyword-scan (not a fixed
    /// split position).
    #[tokio::test]
    async fn schema_get_resolves_an_inline_response_schema_by_dotted_id() {
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "schema",
                "get",
                "commerce.location.verify-address.responses.200.schema",
                "--output",
                "json",
            ])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let rendered: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json");
        assert_eq!(rendered["data"]["type"], json!("object"));
        let properties = rendered["data"]["properties"]
            .as_array()
            .expect("properties array");
        let names: Vec<&str> = properties
            .iter()
            .map(|p| p["name"].as_str().expect("name"))
            .collect();
        assert!(names.contains(&"status"));
        assert!(names.contains(&"data"));
    }

    /// An inline request body's id is a dotted path too — `patchBusiness`'s
    /// top-level schema is `{type: array, items: {$ref: ...}}`, inline at
    /// the top level even though its items aren't.
    #[tokio::test]
    async fn schema_get_resolves_an_inline_request_body_schema_by_dotted_id() {
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "schema",
                "get",
                "patchBusiness.requestBody.schema",
                "--output",
                "json",
            ])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let rendered: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json");
        assert_eq!(rendered["data"]["type"], json!("array"));
        assert!(rendered["data"]["items"].is_object());
    }

    #[tokio::test]
    async fn schema_get_unknown_id_is_a_not_found_error() {
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "schema",
                "get",
                "not-a-defs-name-and-not-a-dotted-id",
                "--output",
                "json",
            ])
            .await;
        assert_ne!(output.exit_code, 0, "{}", output.rendered);
        assert!(
            output.rendered.contains("no schema found for id"),
            "{}",
            output.rendered
        );
    }
}
