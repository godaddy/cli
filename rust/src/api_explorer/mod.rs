use std::sync::OnceLock;

use cli_engine::{
    CommandResult, CommandSpec, GroupSpec, Module, NextActionParam, RuntimeCommandSpec,
    RuntimeGroupSpec, Tier,
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::next_action::next_action;
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
});

// `api endpoint list --domain X` lists endpoints within one domain, so each row
// omits the (redundant) domain field that the cross-domain `api search` emits.
output_schema!(ApiDomainEndpoint {
    "operationId": "string";
    "method": "string";
    "path": "string";
    "summary": "string", optional;
});

output_schema!(ApiOperation {
    "domain": "string";
    "operationId": "string";
    "method": "string";
    "path": "string";
    "summary": "string", optional;
    "description": "string", optional;
    "parameters": "[]object";
    "requestBody": "object", optional;
    "responses": "object";
    "scopes": "[]string";
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
];

fn catalog() -> &'static [Domain] {
    CATALOG.get_or_init(|| {
        DOMAIN_FILES
            .iter()
            .filter_map(|(_, src)| serde_json::from_str::<Domain>(src).ok())
            .collect()
    })
}

fn find_endpoint<'a>(catalog: &'a [Domain], query: &str) -> Option<(&'a Domain, &'a Endpoint)> {
    let q = query.to_lowercase();
    catalog.iter().find_map(|domain| {
        domain.endpoints.iter().find_map(|ep| {
            if ep.operation_id.to_lowercase() == q
                || ep.path.to_lowercase() == q
                || ep.path.to_lowercase().contains(&q)
            {
                Some((domain, ep))
            } else {
                None
            }
        })
    })
}

/// Reads a repeatable string argument, handling both shapes cli-engine
/// produces: a single occurrence is collapsed to a scalar `Value::String`, and
/// only two-or-more become a `Value::Array`. Matching only the array shape
/// silently drops a lone `--scope`/`--field` value, so handle both.
fn string_list(args: &serde_json::Map<String, Value>, key: &str) -> Vec<String> {
    match args.get(key) {
        Some(Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect(),
        Some(Value::String(s)) => vec![s.clone()],
        _ => Vec::new(),
    }
}

/// Split a `--header` value of the form `KEY:VALUE` into trimmed parts.
/// Splits on the first colon only, so values may themselves contain colons
/// (e.g. a URL). Returns None when there is no colon.
fn split_header(raw: &str) -> Option<(&str, &str)> {
    raw.split_once(':').map(|(k, v)| (k.trim(), v.trim()))
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
                if haystack.contains(&q) {
                    Some((domain, ep))
                } else {
                    None
                }
            })
        })
        .collect()
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
        |_ctx| async move {
            let catalog = catalog();
            let domains: Vec<Value> = catalog
                .iter()
                .map(|d| {
                    json!({
                        "domain": d.name,
                        "title": d.title,
                        "description": d.description,
                        "endpoints": d.endpoints.len(),
                        "baseUrl": d.base_url,
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

fn endpoint_list_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_with_context(
        CommandSpec::new("list", "List endpoints within an API domain")
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
            .with_output_schema::<ApiDomainEndpoint>()
            .with_arg(
                clap::Arg::new("domain")
                    .long("domain")
                    .value_name("DOMAIN")
                    .required(true)
                    .help("API domain whose endpoints to list (see `api domain list`)"),
            ),
        |ctx| async move {
            let catalog = catalog();
            let domain_filter = ctx
                .args
                .get("domain")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let domain = catalog
                .iter()
                .find(|d| d.name == domain_filter)
                .ok_or_else(|| {
                    cli_engine::CliCoreError::message(format!("domain '{domain_filter}' not found"))
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

fn describe_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_with_context(
        CommandSpec::new(
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
        .with_output_schema::<ApiOperation>()
        .with_arg(
            clap::Arg::new("endpoint")
                .value_name("ENDPOINT")
                .required(true)
                .help("Operation ID (e.g. createOrder) or path fragment (e.g. /v1/commerce/orders)"),
        ),
        |ctx| async move {
            let query = ctx
                .args
                .get("endpoint")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let catalog = catalog();
            let (domain, ep) = find_endpoint(catalog, query).ok_or_else(|| {
                cli_engine::CliCoreError::message(format!(
                    "no endpoint found matching '{query}' — try `api search {query}`"
                ))
            })?;
            Ok(CommandResult::new(json!({
                "domain": domain.name,
                "operationId": ep.operation_id,
                "method": ep.method,
                "path": ep.path,
                "summary": ep.summary,
                "description": ep.description,
                "parameters": ep.parameters,
                "requestBody": ep.request_body,
                "responses": ep.responses,
                "scopes": ep.scopes,
            }))
            .with_next_actions(vec![
                next_action(
                    format!("api call {} --method {}", ep.path, ep.method),
                    "Make an authenticated call to this endpoint",
                ),
            ]))
        },
    )
}

fn search_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_with_context(
        CommandSpec::new("search", "Search API endpoints by keyword")
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
            .with_output_schema::<ApiEndpoint>()
            .with_arg(
                clap::Arg::new("query")
                    .value_name("QUERY")
                    .required(true)
                    .help("Search term (matches path, operationId, summary, description)"),
            ),
        |ctx| async move {
            let query = ctx.args.get("query").and_then(|v| v.as_str()).unwrap_or("");
            let hits = search_endpoints(catalog(), query);
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

fn extract_json_path<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    let normalized = path.trim_start_matches('.');
    if normalized.is_empty() {
        return Some(value);
    }
    let mut current = value;
    let mut remaining = normalized;
    while !remaining.is_empty() {
        if remaining.starts_with('[') {
            let end = remaining.find(']')?;
            let idx: usize = remaining[1..end].parse().ok()?;
            current = current.get(idx)?;
            remaining = remaining[end + 1..].trim_start_matches('.');
        } else {
            let (key, rest) = match (remaining.find('.'), remaining.find('[')) {
                (Some(d), Some(b)) if b < d => (&remaining[..b], &remaining[b..]),
                (Some(d), _) => (&remaining[..d], &remaining[d + 1..]),
                (None, Some(b)) => (&remaining[..b], &remaining[b..]),
                (None, None) => (remaining, ""),
            };
            current = current.get(key)?;
            remaining = rest;
        }
    }
    Some(current)
}

fn call_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_with_context(
        CommandSpec::new("call", "Make an authenticated API request")
            .with_long(
                "Executes an authenticated HTTP request against any GoDaddy API endpoint. \
                 Required OAuth scopes are resolved automatically: the catalog is searched \
                 for the endpoint path and its declared scopes are merged with any explicit \
                 `--scope` flags, then a credential with exactly those scopes is obtained \
                 before the request is sent. Supply the request body as raw JSON (`--body \
                 '{...}'`), as individual fields (`--field key=value`, repeatable), or \
                 from a JSON file (`--file body.json`); `--file` takes precedence over \
                 `--body`, and `--field` values are merged on top of either. Use `--query \
                 .path[0].field` to extract a value from the response JSON, and `--include` \
                 to see response headers alongside the body. Use `api describe <endpoint>` \
                 first to inspect required parameters and scopes.",
            )
            .with_system("api")
            .with_tier(Tier::Mutate)
            .with_arg(
                clap::Arg::new("endpoint")
                    .value_name("ENDPOINT")
                    .required(true)
                    .help("Relative API path (e.g. /v1/commerce/stores/{storeId}/orders)"),
            )
            .with_arg(
                clap::Arg::new("method")
                    .long("method")
                    .short('X')
                    .value_name("METHOD")
                    .default_value("GET")
                    .help("HTTP method"),
            )
            .with_arg(
                clap::Arg::new("body")
                    .long("body")
                    .short('d')
                    .value_name("JSON")
                    .help("Request body as raw JSON string"),
            )
            .with_arg(
                clap::Arg::new("field")
                    .long("field")
                    .short('f')
                    .value_name("KEY=VALUE")
                    .num_args(0..)
                    .help("Add a field to the request body (key=value, repeatable)"),
            )
            .with_arg(
                clap::Arg::new("file")
                    .long("file")
                    .short('F')
                    .value_name("PATH")
                    .help("Read request body from a JSON file"),
            )
            .with_arg(
                clap::Arg::new("header")
                    .long("header")
                    .short('H')
                    .value_name("KEY:VALUE")
                    .num_args(0..)
                    .help("Extra request headers"),
            )
            .with_arg(
                clap::Arg::new("query")
                    .long("query")
                    .short('q')
                    .value_name("PATH")
                    .help("Extract a value from the response JSON (e.g. .data[0].id)"),
            )
            .with_arg(
                clap::Arg::new("include")
                    .long("include")
                    .short('i')
                    .action(clap::ArgAction::SetTrue)
                    .help("Include response headers in output"),
            )
            .with_arg(
                clap::Arg::new("scope")
                    .long("scope")
                    .short('s')
                    .value_name("SCOPE")
                    // One value per occurrence, repeatable (`--scope a --scope b`).
                    // Append (vs num_args(1..)) avoids greedily consuming the
                    // ENDPOINT positional and still rejects a bare `--scope`.
                    .action(clap::ArgAction::Append)
                    .help("Additional required OAuth scope(s), merged with the endpoint's"),
            ),
        |ctx| async move {
            let endpoint = ctx
                .args
                .get("endpoint")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let method = ctx
                .args
                .get("method")
                .and_then(|v| v.as_str())
                .unwrap_or("GET");
            // Required scopes = explicit --scope flags, plus the matched catalog
            // endpoint's declared scopes (best-effort: a concrete request path
            // may not match a templated catalog path, in which case only --scope
            // contributes). These drive OAuth scope step-up at credential time.
            let flag_scopes = string_list(&ctx.args, "scope");
            let endpoint_scopes = find_endpoint(catalog(), endpoint)
                .map(|(_, ep)| ep.scopes.as_slice())
                .unwrap_or(&[]);
            let required = merge_required_scopes(flag_scopes, endpoint_scopes);

            let token = ctx.credential_with_scopes(&required).await?.token;
            let base_url = crate::application::client::api_url_for_env(&ctx.middleware.env);
            let url = format!("{base_url}{endpoint}");

            // Build request body: -F file > -d body, then merge -f fields on top
            let mut request_body: Option<Value> = None;

            if let Some(file_path) = ctx.args.get("file").and_then(|v| v.as_str()) {
                let content = std::fs::read_to_string(file_path).map_err(|e| {
                    cli_engine::CliCoreError::message(format!(
                        "failed to read file '{file_path}': {e}"
                    ))
                })?;
                request_body = Some(serde_json::from_str(&content).map_err(|e| {
                    cli_engine::CliCoreError::message(format!("invalid JSON in '{file_path}': {e}"))
                })?);
            } else if let Some(body_str) = ctx.args.get("body").and_then(|v| v.as_str()) {
                request_body = Some(serde_json::from_str(body_str).map_err(|e| {
                    cli_engine::CliCoreError::message(format!("invalid JSON body: {e}"))
                })?);
            }

            let fields = string_list(&ctx.args, "field");
            if !fields.is_empty() {
                let body = request_body.get_or_insert_with(|| json!({}));
                for s in &fields {
                    let eq = s.find('=').ok_or_else(|| {
                        cli_engine::CliCoreError::message(format!(
                            "invalid field format '{s}': expected key=value"
                        ))
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
                .request(
                    method.parse().map_err(|_| {
                        cli_engine::CliCoreError::message(format!("invalid HTTP method: {method}"))
                    })?,
                    &url,
                )
                .bearer_auth(&token)
                .header("x-request-id", uuid::Uuid::new_v4().to_string());

            // Apply user-supplied `--header KEY:VALUE` values (repeatable). These
            // were parsed but never attached to the request, so endpoints that
            // require a header — e.g. the GraphQL commerce domains need `x-store-id`
            // — could not be called at all.
            for h in string_list(&ctx.args, "header") {
                let (key, val) = split_header(&h).ok_or_else(|| {
                    cli_engine::CliCoreError::message(format!(
                        "invalid header '{h}': expected KEY:VALUE"
                    ))
                })?;
                req = req.header(key, val);
            }

            if let Some(body) = request_body {
                req = req.json(&body);
            }

            let request = req
                .build()
                .map_err(|e| cli_engine::CliCoreError::message(e.to_string()))?;
            cli_engine::transport::debug_log_reqwest_request(&request);
            let resp = client
                .execute(request)
                .await
                .map_err(|e| cli_engine::CliCoreError::message(e.to_string()))?;

            let status_code = resp.status();
            let status_text = status_code.canonical_reason().unwrap_or("").to_owned();
            let response_headers_raw = resp.headers().clone();
            let include_headers = ctx
                .args
                .get("include")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let body_bytes = resp
                .bytes()
                .await
                .map_err(|e| cli_engine::CliCoreError::message(e.to_string()))?;
            cli_engine::transport::debug_log_reqwest_response(
                status_code,
                &response_headers_raw,
                &body_bytes,
            );

            let status = status_code.as_u16();
            let response_headers: Option<serde_json::Map<String, Value>> = if include_headers {
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
                return Err(cli_engine::CliCoreError::message(format!(
                    "GraphQL request returned {} error(s):\n{}",
                    errors.len(),
                    serde_json::to_string_pretty(&json!(errors)).unwrap_or_default(),
                )));
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
                return Err(cli_engine::CliCoreError::message(format!(
                    "403 Forbidden — the authorized token is missing required scope(s): {}. \
                     Re-run `gddy auth login {login_hint}` and try again.",
                    required.join(", "),
                )));
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
                return Err(cli_engine::CliCoreError::message(format!(
                    "{status} {status_text}\n{detail}"
                )));
            }

            let query_path = ctx.args.get("query").and_then(|v| v.as_str());
            let output = if let Some(path) = query_path {
                extract_json_path(&body, path)
                    .cloned()
                    .unwrap_or(json!(null))
            } else {
                body
            };

            // Identify the call and its outcome in the result envelope.
            let mut result = json!({
                "endpoint": endpoint,
                "method": method,
                "status": status,
                "status_text": status_text,
                "data": output,
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
    use super::merge_required_scopes;

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
    fn string_list_handles_scalar_array_and_missing() {
        use serde_json::json;
        // A single occurrence serializes to a scalar String (the bug case).
        let mut args = serde_json::Map::new();
        args.insert("scope".to_owned(), json!("solo"));
        assert_eq!(super::string_list(&args, "scope"), v(&["solo"]));
        // Two-or-more serialize to an array.
        args.insert("scope".to_owned(), json!(["a", "b"]));
        assert_eq!(super::string_list(&args, "scope"), v(&["a", "b"]));
        // Missing key.
        assert!(super::string_list(&args, "absent").is_empty());
    }
}
