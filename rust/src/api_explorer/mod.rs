use std::sync::OnceLock;

use cli_engine::{
    CommandResult, CommandSpec, GroupSpec, Module, NextAction, NextActionParam, RuntimeCommandSpec,
    RuntimeGroupSpec, Tier,
};
use serde::Deserialize;
use serde_json::{Value, json};

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
    Module::new("Contracts", |_ctx| {
        RuntimeGroupSpec::new(GroupSpec::new(
            "api",
            "Explore and call GoDaddy API endpoints",
        ))
        .with_command(list_command())
        .with_command(describe_command())
        .with_command(search_command())
        .with_command(call_command())
    })
}

fn list_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_with_context(
        CommandSpec::new("list", "List all API domains")
            .with_system("api")
            .with_tier(Tier::Read)
            .no_auth(true)
            .with_default_fields("domain,title,endpoints,baseUrl")
            .with_arg(
                clap::Arg::new("domain")
                    .long("domain")
                    .value_name("DOMAIN")
                    .help("Show endpoints within a specific domain"),
            ),
        |ctx| async move {
            let catalog = catalog();
            if let Some(domain_filter) = ctx.args.get("domain").and_then(|v| v.as_str()) {
                let domain = catalog
                    .iter()
                    .find(|d| d.name == domain_filter)
                    .ok_or_else(|| {
                        cli_engine::CliCoreError::message(format!(
                            "domain '{domain_filter}' not found"
                        ))
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
                return Ok(CommandResult::new(json!(endpoints)).with_next_actions(vec![
                    NextAction::new(
                        "api describe <operationId>",
                        "Get full details for an endpoint",
                    )
                    .with_param("operationId", NextActionParam::required()),
                ]));
            }

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
                NextAction::new(
                    "api list --domain <domain>",
                    "List endpoints in a specific domain",
                )
                .with_param("domain", NextActionParam::required()),
                NextAction::new("api search <query>", "Search across all endpoints"),
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
        .with_system("api")
        .with_tier(Tier::Read)
        .no_auth(true)
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
                NextAction::new(
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
            .with_system("api")
            .with_tier(Tier::Read)
            .no_auth(true)
            .with_default_fields("domain,method,path,summary")
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
                NextAction::new(
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
            let flag_scopes: Vec<String> = ctx
                .args
                .get("scope")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default();
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

            if let Some(fields) = ctx.args.get("field").and_then(|v| v.as_array())
                && !fields.is_empty()
            {
                let body = request_body.get_or_insert_with(|| json!({}));
                for field in fields {
                    if let Some(s) = field.as_str() {
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
            }

            let mut req = crate::application::client::make_http_client()
                .request(
                    method.parse().map_err(|_| {
                        cli_engine::CliCoreError::message(format!("invalid HTTP method: {method}"))
                    })?,
                    &url,
                )
                .bearer_auth(&token)
                .header("x-request-id", uuid::Uuid::new_v4().to_string());

            if let Some(body) = request_body {
                req = req.json(&body);
            }

            let resp = req
                .send()
                .await
                .map_err(|e| cli_engine::CliCoreError::message(e.to_string()))?;

            let status = resp.status().as_u16();
            let include_headers = ctx
                .args
                .get("include")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let response_headers: Option<serde_json::Map<String, Value>> = if include_headers {
                Some(
                    resp.headers()
                        .iter()
                        .filter_map(|(k, v)| v.to_str().ok().map(|s| (k.to_string(), json!(s))))
                        .collect(),
                )
            } else {
                None
            };

            let body: Value = resp.json().await.unwrap_or(json!(null));

            // Scope step-up already ran up front (the token was requested with
            // `required`). A 403 here means the granted token still lacks a
            // required scope — surface it rather than silently returning the body.
            if status == 403 && !required.is_empty() {
                return Err(cli_engine::CliCoreError::message(format!(
                    "403 Forbidden — the authorized token is missing required scope(s): {}. \
                     Re-run `gddy auth login --scope {}` and try again.",
                    required.join(", "),
                    required.join(" ")
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

            let mut result = json!({ "status": status, "body": output });
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
}
