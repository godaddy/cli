use std::sync::OnceLock;

use cli_engine::{
    CommandResult, CommandSpec, GroupSpec, Module, NextActionParam, RuntimeCommandSpec,
    RuntimeGroupSpec, Tier,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::next_action::{next_action, required_value};
use crate::output_schema::output_schema;

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

// `api endpoint list --domain X` lists endpoints within one domain, so each row
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
    "parameters": "[]object";
    "requestBody": "object", optional;
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
        // list`, `api search`, `api describe` — sees the same stable order.
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
/// method. Used by `api describe`'s primary resolution step, distinct from
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

/// Builds the `{message, matches}` response for an ambiguous resolution —
/// shared by `api describe`'s exact-match step (same path, multiple methods)
/// and its fuzzy fallback (same query, multiple unrelated endpoints) — with
/// a `next_action` per candidate that carries both `endpoint` and `method`,
/// since candidates sharing one path are only distinguishable by method.
fn multi_match_result(query: &str, hits: &[(&Domain, &Endpoint)]) -> CommandResult {
    let matches: Vec<Value> = hits
        .iter()
        .map(|(domain, ep)| {
            json!({
                "operationId": ep.operation_id,
                "method": ep.method,
                "path": ep.path,
                "summary": ep.summary,
                "domain": domain.name,
            })
        })
        .collect();
    let next_actions = hits
        .iter()
        .map(|(_, ep)| {
            next_action(
                "api describe <endpoint> --method <method>",
                format!("{} {} — {}", ep.method, ep.path, ep.summary),
            )
            .with_param("endpoint", required_value(ep.path.clone()))
            .with_param("method", required_value(ep.method.clone()))
        })
        .collect();
    CommandResult::new(json!({
        "message": format!("Multiple endpoints match '{query}'. Be more specific:"),
        "matches": matches,
    }))
    .with_next_actions(next_actions)
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
// top-level property names/types for `api describe`, so a caller sees a
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

struct RequestBodySummary {
    required: bool,
    content_type: Option<String>,
    description: Option<String>,
    schema: Option<Vec<SchemaSummaryProperty>>,
}

struct ResponseSummary {
    description: String,
    schema: Option<Vec<SchemaSummaryProperty>>,
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

fn summarize_request_body(
    request_body: Option<&Value>,
    defs: &Map<String, Value>,
) -> Option<RequestBodySummary> {
    let obj = request_body?.as_object()?;
    Some(RequestBodySummary {
        required: obj
            .get("required")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        content_type: obj
            .get("contentType")
            .and_then(Value::as_str)
            .map(str::to_owned),
        description: obj
            .get("description")
            .and_then(Value::as_str)
            .map(str::to_owned),
        schema: summarize_schema(obj.get("schema"), defs),
    })
}

fn summarize_responses(
    responses: &Value,
    defs: &Map<String, Value>,
) -> Option<Vec<(String, ResponseSummary)>> {
    let obj = responses.as_object()?;
    Some(
        obj.iter()
            .map(|(status, resp)| {
                let description = resp
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned();
                let schema = summarize_schema(resp.get("schema"), defs);
                (
                    status.clone(),
                    ResponseSummary {
                        description,
                        schema,
                    },
                )
            })
            .collect(),
    )
}

// ---------------------------------------------------------------------------
// GraphQL summarization — condenses a domain's embedded GraphQL schema
// (parsed at catalog-build time) into a per-operation summary for
// `api describe`. Ported from the original TypeScript CLI's
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
                     against any endpoint. Use `api domain list` / `api endpoint list` to \
                     discover available operations, `api describe` to inspect parameters, \
                     and `api call` to execute a request with automatic OAuth scope handling.",
            ),
        )
        .with_group(
            RuntimeGroupSpec::new(GroupSpec::new("domain", "Browse API domains").with_long(
                "Browse the top-level API domains available in the embedded catalog. \
                     Each domain groups a related set of endpoints under a shared base URL. \
                     Use `api endpoint list --domain <domain>` to see the endpoints \
                     within a specific domain.",
            ))
            .with_command(domain_list_command()),
        )
        .with_group(
            RuntimeGroupSpec::new(
                GroupSpec::new("endpoint", "Browse API endpoints").with_long(
                    "Browse the endpoints within a single API domain. \
                     Use `api domain list` first to find available domain names, \
                     then `api describe <operationId>` to inspect full parameter \
                     and schema details for an individual endpoint.",
                ),
            )
            .with_command(endpoint_list_command()),
        )
        .with_command(describe_command())
        .with_command(search_command())
        .with_command(call_command())
    })
}

fn domain_list_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_with_context(
        CommandSpec::new("list", "List all API domains")
            .with_long(
                "Lists every API domain in the embedded catalog, together with the number \
                 of endpoints and base URL for each. No authentication is required. \
                 Use `api endpoint list --domain <domain>` to drill into a specific domain, \
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
                    "api endpoint list --domain <domain>",
                    "List endpoints in a specific domain",
                )
                .with_param("domain", NextActionParam::required()),
                next_action("api search <query>", "Search across all endpoints"),
            ]))
        },
    )
}

#[derive(Debug, Clone, clap::Args)]
struct EndpointListArgs {
    /// API domain whose endpoints to list (see `api domain list`).
    #[arg(long, value_name = "DOMAIN")]
    domain: String,
}

fn endpoint_list_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<EndpointListArgs, _, _, _>(
        CommandSpec::from_args::<EndpointListArgs>("list", "List endpoints within an API domain")
            .with_long(
                "Lists every endpoint in one API domain, showing the operation ID, HTTP \
                 method, path, and summary. Use `api domain list` to find available domain \
                 names, `api describe <operationId>` to view full parameter details, and \
                 `api call <path>` to execute a request.",
            )
            .with_system("api")
            .with_tier(Tier::Read)
            .no_auth(true)
            .with_default_fields("operationId,method,path,summary")
            .with_output_schema::<ApiDomainEndpoint>(),
        |_ctx, args: EndpointListArgs| async move {
            let catalog = catalog();
            let domain_filter = args.domain;
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
                    "api describe <operationId>",
                    "Get full details for an endpoint",
                )
                .with_param("operationId", NextActionParam::required()),
            ]))
        },
    )
}

#[derive(Debug, Clone, clap::Args)]
struct DescribeArgs {
    /// Operation ID (e.g. createOrder) or path fragment (e.g. /v1/commerce/orders).
    #[arg(value_name = "ENDPOINT")]
    endpoint: String,

    /// Filter to a specific HTTP method (GET, POST, PUT, PATCH, DELETE).
    #[arg(long, short = 'm', value_name = "METHOD")]
    method: Option<String>,
}

fn describe_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<DescribeArgs, _, _, _>(
        CommandSpec::from_args::<DescribeArgs>(
            "describe",
            "Show schema and parameters for an endpoint",
        )
        .with_long(
            "Shows the full details of one API endpoint: HTTP method, path, required and \
             optional parameters, request body schema, response shapes, and declared OAuth \
             scopes. Accepts an operation ID (e.g. createOrder) or a path fragment \
             (e.g. /v1/commerce/orders). No authentication is required.",
        )
        .with_system("api")
        .with_tier(Tier::Read)
        .no_auth(true)
        .with_output_schema::<ApiOperation>(),
        |ctx, args: DescribeArgs| async move {
            let query = args.endpoint.as_str();
            let method_filter = args.method.map(|m| m.to_uppercase());
            let catalog = catalog();

            // Exact operationId/path/template match (optionally narrowed by
            // --method) first; a miss falls back to fuzzy substring search,
            // which ignores --method entirely (matches the original CLI).
            // Either step can legitimately produce more than one candidate
            // (e.g. GET/POST sharing a path with no --method given) — both
            // are treated as the same kind of ambiguity.
            let exact_hits = find_endpoint_exact(catalog, query, method_filter.as_deref());
            let (domain, ep) = match exact_hits.len() {
                1 => exact_hits[0],
                0 => {
                    let hits = search_endpoints(catalog, query);
                    match hits.len() {
                        0 => {
                            return Err(crate::error::GddyError::not_found(format!(
                                "no endpoint found matching '{query}' — try `gddy api search {query}`"
                            ))
                            .with_fix(format!(
                                "Run: gddy api search {query} or gddy api endpoint list"
                            ))
                            .into_cli_error());
                        }
                        1 => hits[0],
                        _ => return Ok(multi_match_result(query, &hits)),
                    }
                }
                _ => return Ok(multi_match_result(query, &exact_hits)),
            };

            // Env-aware base URL, matching `domain_list_command`/`call_command`
            // — the catalog's `baseUrl` is a static, prod-shaped value; this
            // resolves it against the active environment the same way an
            // actual `api call` to this endpoint would.
            let base_url = crate::environments::resolve_catalog_base_url(
                &domain.name,
                &domain.base_url,
                &ctx.middleware.env,
            );

            let summarized_request = summarize_request_body(ep.request_body.as_ref(), &domain.defs);
            let summarized_responses = summarize_responses(&ep.responses, &domain.defs);

            let request_body_json = summarized_request.map(|r| {
                json!({
                    "required": r.required,
                    "contentType": r.content_type,
                    "description": r.description,
                    "schema": r.schema,
                })
            });
            let responses_json: Option<Map<String, Value>> = summarized_responses.map(|list| {
                list.into_iter()
                    .map(|(status, r)| {
                        (
                            status,
                            json!({ "description": r.description, "schema": r.schema }),
                        )
                    })
                    .collect()
            });

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

            Ok(CommandResult::new(json!({
                "domain": domain.name,
                "baseUrl": base_url,
                "operationId": ep.operation_id,
                "method": ep.method,
                "path": ep.path,
                "fullPath": full_path,
                "summary": ep.summary,
                "description": ep.description,
                "parameters": ep.parameters,
                "requestBody": request_body_json,
                "responses": responses_json,
                "scopes": ep.scopes,
                "graphql": ep.graphql.as_ref().map(summarize_graphql_schema),
            }))
            .with_next_actions(vec![next_action(
                format!("api call {} --method {}", ep.path, ep.method),
                "Make an authenticated call to this endpoint",
            )]))
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
                 `api describe <operationId>` to inspect a result in full detail.",
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
                    "api describe <operationId>",
                    "Get full details for a result",
                )
                .with_param("operationId", NextActionParam::required()),
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
                 `api describe <endpoint>` first to inspect required parameters and scopes.",
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
                     use `api describe {endpoint}` to find the concrete path"
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
    /// search`, `api describe`) sees the same stable, alphabetical order.
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
    // api describe — exact/method/fuzzy resolution, schema + GraphQL summary
    // -----------------------------------------------------------------------

    fn describe_cli() -> Cli {
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
    async fn describe_exact_operation_id_match() {
        // Pin `--env` explicitly: `fullPath` depends on env-resolved base
        // URL, and the default env is read from ambient local config
        // (`gdenv`), so a bare run isn't hermetic across machines/CI.
        let output = describe_cli()
            .run([
                "gddy",
                "api",
                "describe",
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
    async fn describe_full_path_is_hostless_in_a_non_prod_env() {
        let output = describe_cli()
            .run([
                "gddy",
                "api",
                "describe",
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
    async fn describe_exact_match_narrowed_by_method_flag() {
        let output = describe_cli()
            .run([
                "gddy",
                "api",
                "describe",
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
    async fn describe_method_filter_is_ignored_during_fuzzy_fallback() {
        let output = describe_cli()
            .run([
                "gddy",
                "api",
                "describe",
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
    async fn describe_single_fuzzy_match_resolves_transparently() {
        let output = describe_cli()
            .run([
                "gddy",
                "api",
                "describe",
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

    #[tokio::test]
    async fn describe_multiple_fuzzy_matches_lists_candidates() {
        let output = describe_cli()
            .run(["gddy", "api", "describe", "/location", "--output", "json"])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let rendered: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json");
        assert!(
            rendered["data"]["message"]
                .as_str()
                .unwrap_or_default()
                .starts_with("Multiple endpoints match"),
            "{}",
            output.rendered
        );
        let matches = rendered["data"]["matches"]
            .as_array()
            .expect("matches array");
        assert_eq!(matches.len(), 2);
        assert_eq!(
            rendered["next_actions"]
                .as_array()
                .expect("next_actions array")
                .len(),
            2
        );
    }

    /// Many catalog paths are shared by several endpoints that only differ
    /// by method (here: `GET /businesses` and `POST /businesses`) — without
    /// `--method`, exact-match resolution must treat that the same as an
    /// ambiguous fuzzy match, not silently pick whichever one it saw first.
    #[tokio::test]
    async fn describe_exact_path_shared_by_multiple_methods_is_ambiguous() {
        let output = describe_cli()
            .run(["gddy", "api", "describe", "/businesses", "--output", "json"])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let rendered: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json");
        let matches = rendered["data"]["matches"]
            .as_array()
            .expect("matches array");
        assert_eq!(matches.len(), 2, "{}", output.rendered);
        let next_actions = rendered["next_actions"].as_array().expect("next_actions");
        assert_eq!(next_actions.len(), 2);
        // Candidates sharing one path are only distinguishable by method, so
        // each next_action must carry both params, not just `endpoint`.
        for action in next_actions {
            assert!(action["params"]["endpoint"]["value"].is_string());
            assert!(action["params"]["method"]["value"].is_string());
        }
    }

    /// The same ambiguity, resolved by adding `--method`.
    #[tokio::test]
    async fn describe_exact_path_shared_by_multiple_methods_resolves_with_method_flag() {
        let output = describe_cli()
            .run([
                "gddy",
                "api",
                "describe",
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
    async fn describe_concrete_path_resolves_via_template_matching() {
        let output = describe_cli()
            .run([
                "gddy",
                "api",
                "describe",
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
    async fn describe_zero_matches_is_an_error() {
        let output = describe_cli()
            .run([
                "gddy",
                "api",
                "describe",
                "totally-fake-endpoint-xyz",
                "--output",
                "json",
            ])
            .await;
        assert_ne!(output.exit_code, 0, "{}", output.rendered);
        assert!(
            output.rendered.contains("no endpoint found matching"),
            "{}",
            output.rendered
        );
    }

    /// Most catalog request bodies are a bare `$ref` with no inline
    /// `properties` (63/82 in the embedded catalog) — `createBusiness`'s is
    /// `{"$ref": "#/$defs/Business"}`. Without $ref resolution this
    /// summarizes to `null`; with it, the real fields show up.
    #[tokio::test]
    async fn describe_resolves_a_pure_ref_request_body_against_defs() {
        let output = describe_cli()
            .run([
                "gddy",
                "api",
                "describe",
                "createBusiness",
                "--output",
                "json",
            ])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let rendered: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json");
        let schema = rendered["data"]["requestBody"]["schema"]
            .as_array()
            .expect("requestBody.schema resolves to a property list, not null");
        let active_since = schema
            .iter()
            .find(|p| p["name"] == "activeSince")
            .expect("activeSince property");
        assert_eq!(active_since["type"], json!("string(date-time)"));
    }

    #[tokio::test]
    async fn describe_graphql_endpoint_gets_summary() {
        let output = describe_cli()
            .run([
                "gddy",
                "api",
                "describe",
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
}
