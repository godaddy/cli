use std::{collections::HashMap, path::Path};

use serde::Serialize;
use serde_json::Value;

use crate::graphql::{
    CatalogGraphqlArgument, CatalogGraphqlOperation, CatalogGraphqlSchema, load_graphql_schema,
};

const HTTP_METHODS: &[&str] = &[
    "get", "post", "put", "patch", "delete", "options", "head", "trace",
];

// ---------------------------------------------------------------------------
// Output catalog types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub(crate) struct CatalogParameter {
    name: String,
    #[serde(rename = "in")]
    location: String,
    required: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    schema: Option<Value>,
}

#[derive(Debug, Serialize)]
pub(crate) struct CatalogRequestBody {
    required: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(rename = "contentType")]
    content_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    schema: Option<Value>,
}

#[derive(Debug, Serialize)]
pub(crate) struct CatalogResponse {
    pub(crate) description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) schema: Option<Value>,
}

#[derive(Debug, Serialize)]
pub(crate) struct CatalogEndpoint {
    #[serde(rename = "operationId")]
    pub(crate) operation_id: String,
    pub(crate) method: String,
    pub(crate) path: String,
    pub(crate) summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) parameters: Option<Vec<CatalogParameter>>,
    #[serde(rename = "requestBody", skip_serializing_if = "Option::is_none")]
    pub(crate) request_body: Option<CatalogRequestBody>,
    pub(crate) responses: HashMap<String, CatalogResponse>,
    pub(crate) scopes: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) graphql: Option<CatalogGraphqlSchema>,
}

#[derive(Debug, Serialize)]
pub(crate) struct CatalogDomain {
    pub(crate) name: String,
    pub(crate) title: String,
    pub(crate) description: String,
    pub(crate) version: String,
    #[serde(rename = "baseUrl")]
    pub(crate) base_url: String,
    pub(crate) endpoints: Vec<CatalogEndpoint>,
}

// ---------------------------------------------------------------------------
// Scope normalization
// ---------------------------------------------------------------------------

fn normalize_scope(scope: &str) -> String {
    let s = scope.trim();
    if s.is_empty() {
        return s.to_owned();
    }

    // urn:godaddy:services:commerce.X:Y
    if let Some(rest) = s.strip_prefix("urn:godaddy:services:commerce.")
        && let Some(colon) = rest.find(':')
    {
        let domain = rest[..colon].to_lowercase();
        let action = normalize_scope_action(&rest[colon + 1..]);
        return format!("commerce.{domain}:{action}");
    }

    // https://uri.godaddy.com/services/commerce/X/Y
    if let Some(rest) = s.strip_prefix("https://uri.godaddy.com/services/commerce/")
        && let Some(slash) = rest.rfind('/')
    {
        let domain = rest[..slash].to_lowercase();
        let action = normalize_scope_action(&rest[slash + 1..]);
        return format!("commerce.{domain}:{action}");
    }

    // commerce.X:Y — already in target format, just normalize action
    let s_lower = s.to_lowercase();
    if let Some(rest) = s_lower.strip_prefix("commerce.")
        && let Some(colon) = rest.find(':')
    {
        let domain = rest[..colon].to_owned();
        let action = normalize_scope_action(&rest[colon + 1..]);
        return format!("commerce.{domain}:{action}");
    }

    s.to_owned()
}

fn normalize_scope_action(action: &str) -> String {
    let a = action.trim().to_lowercase();
    if a == "read-write" {
        "write".to_owned()
    } else {
        a
    }
}

fn extract_scopes(security: &Value) -> Vec<String> {
    let arr = match security.as_array() {
        Some(a) => a,
        None => return Vec::new(),
    };
    let mut scopes = Vec::new();
    for entry in arr {
        if let Some(map) = entry.as_object() {
            for scope_list in map.values() {
                if let Some(list) = scope_list.as_array() {
                    for s in list {
                        if let Some(raw) = s.as_str() {
                            let normalized = normalize_scope(raw);
                            if !normalized.is_empty() && !scopes.contains(&normalized) {
                                scopes.push(normalized);
                            }
                        }
                    }
                }
            }
        }
    }
    scopes
}

// ---------------------------------------------------------------------------
// OpenAPI processing
// ---------------------------------------------------------------------------

fn resolve_base_url(servers: &Value) -> String {
    let arr = match servers.as_array() {
        Some(a) if !a.is_empty() => a,
        _ => return String::new(),
    };
    let server = &arr[0];
    let mut url = server
        .get("url")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_owned();
    if let Some(vars) = server.get("variables").and_then(|v| v.as_object()) {
        for (key, var) in vars {
            if let Some(default) = var.get("default").and_then(|v| v.as_str()) {
                url = url.replace(&format!("{{{key}}}"), default);
            }
        }
    }
    url
}

fn process_parameter(param: &Value) -> Option<CatalogParameter> {
    let name = param.get("name")?.as_str()?.to_owned();
    let location = param.get("in")?.as_str()?.to_owned();
    let required = param
        .get("required")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let description = param
        .get("description")
        .and_then(|v| v.as_str())
        .map(str::to_owned);
    let schema = param.get("schema").cloned();
    Some(CatalogParameter {
        name,
        location,
        required,
        description,
        schema,
    })
}

fn process_request_body(rb: &Value) -> Option<CatalogRequestBody> {
    // Skip $ref objects that weren't resolved
    if rb.get("$ref").is_some() {
        return None;
    }
    let required = rb
        .get("required")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let description = rb
        .get("description")
        .and_then(|v| v.as_str())
        .map(str::to_owned);
    let content = rb.get("content")?.as_object()?;
    let content_type = content
        .keys()
        .next()
        .cloned()
        .unwrap_or_else(|| "application/json".to_owned());
    let schema = content
        .get(&content_type)
        .and_then(|ct| ct.get("schema"))
        .cloned();
    Some(CatalogRequestBody {
        required,
        description,
        content_type,
        schema,
    })
}

fn process_responses(responses: &Value) -> HashMap<String, CatalogResponse> {
    let mut map = HashMap::new();
    let obj = match responses.as_object() {
        Some(o) => o,
        None => return map,
    };
    for (status, resp) in obj {
        if let Some(ref_val) = resp.get("$ref").and_then(|v| v.as_str()) {
            let schema = if ref_val.starts_with("#/$defs/") {
                Some(resp.clone())
            } else {
                None
            };
            map.insert(
                status.clone(),
                CatalogResponse {
                    description: format!("See {ref_val}"),
                    schema,
                },
            );
            continue;
        }
        let description = resp
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_owned();
        let schema = resp
            .get("content")
            .and_then(|c| c.as_object())
            .and_then(|c| c.values().next())
            .and_then(|ct| ct.get("schema"))
            .cloned();
        map.insert(
            status.clone(),
            CatalogResponse {
                description,
                schema,
            },
        );
    }
    map
}

fn operation_id_fallback(method: &str, path: &str) -> String {
    let slug: String = path
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect::<String>()
        .split('_')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("_");
    format!("{method}_{slug}")
}

#[allow(clippy::too_many_arguments)]
fn process_operation(
    _spec: &Value,
    spec_file: &Path,
    method: &str,
    path_str: &str,
    operation: &Value,
    path_params: &[Value],
    common_types_dir: Option<&Path>,
    graphql_cache: &mut HashMap<String, CatalogGraphqlSchema>,
) -> CatalogEndpoint {
    let operation_id = operation
        .get("operationId")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_owned();
    let operation_id = if operation_id.is_empty() {
        operation_id_fallback(method, path_str)
    } else {
        operation_id
    };

    let summary = operation
        .get("summary")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_owned();
    let description = operation
        .get("description")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_owned);

    // Merge path-level and operation-level parameters
    let op_params = operation
        .get("parameters")
        .and_then(|v| v.as_array())
        .map(|a| a.as_slice())
        .unwrap_or(&[]);
    let all_params: Vec<&Value> = path_params.iter().chain(op_params.iter()).collect();
    let parameters: Vec<CatalogParameter> = all_params
        .iter()
        .filter_map(|p| {
            // Skip $ref params that weren't resolved
            if p.get("$ref").is_some() {
                return None;
            }
            process_parameter(p)
        })
        .collect();
    let parameters = if parameters.is_empty() {
        None
    } else {
        Some(parameters)
    };

    let request_body = operation.get("requestBody").and_then(process_request_body);
    let responses = operation
        .get("responses")
        .map(process_responses)
        .unwrap_or_default();

    let scopes = operation
        .get("security")
        .map(extract_scopes)
        .unwrap_or_default();

    // GraphQL schema extension
    let graphql = operation
        .get("x-godaddy-graphql-schema")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .and_then(|schema_ref| {
            let spec_dir = spec_file.parent().unwrap_or(spec_file);
            let resolved = spec_dir.join(schema_ref);
            let cache_key = resolved.to_string_lossy().into_owned();
            if let Some(cached) = graphql_cache.get(&cache_key) {
                // Return a clone with original schema_ref
                Some(CatalogGraphqlSchema {
                    schema_ref: schema_ref.to_owned(),
                    operation_count: cached.operation_count,
                    operations: cached
                        .operations
                        .iter()
                        .map(|op| CatalogGraphqlOperation {
                            name: op.name.clone(),
                            kind: op.kind.clone(),
                            return_type: op.return_type.clone(),
                            description: op.description.clone(),
                            deprecated: op.deprecated,
                            deprecation_reason: op.deprecation_reason.clone(),
                            args: op
                                .args
                                .iter()
                                .map(|a| CatalogGraphqlArgument {
                                    name: a.name.clone(),
                                    arg_type: a.arg_type.clone(),
                                    required: a.required,
                                    description: a.description.clone(),
                                    default_value: a.default_value.clone(),
                                })
                                .collect(),
                        })
                        .collect(),
                })
            } else {
                match load_graphql_schema(&resolved, schema_ref, common_types_dir) {
                    Ok(gql) => {
                        graphql_cache.insert(
                            cache_key,
                            CatalogGraphqlSchema {
                                schema_ref: gql.schema_ref.clone(),
                                operation_count: gql.operation_count,
                                operations: gql
                                    .operations
                                    .iter()
                                    .map(|op| CatalogGraphqlOperation {
                                        name: op.name.clone(),
                                        kind: op.kind.clone(),
                                        return_type: op.return_type.clone(),
                                        description: op.description.clone(),
                                        deprecated: op.deprecated,
                                        deprecation_reason: op.deprecation_reason.clone(),
                                        args: op
                                            .args
                                            .iter()
                                            .map(|a| CatalogGraphqlArgument {
                                                name: a.name.clone(),
                                                arg_type: a.arg_type.clone(),
                                                required: a.required,
                                                description: a.description.clone(),
                                                default_value: a.default_value.clone(),
                                            })
                                            .collect(),
                                    })
                                    .collect(),
                            },
                        );
                        Some(gql)
                    }
                    Err(e) => {
                        eprintln!("WARNING: failed to load GraphQL schema {schema_ref}: {e}");
                        None
                    }
                }
            }
        });

    CatalogEndpoint {
        operation_id,
        method: method.to_uppercase(),
        path: path_str.to_owned(),
        summary,
        description,
        parameters,
        request_body,
        responses,
        scopes,
        graphql,
    }
}

pub(crate) fn process_spec(
    spec: Value,
    domain: &str,
    spec_file: &Path,
    common_types_dir: Option<&Path>,
) -> CatalogDomain {
    let base_url = spec
        .get("servers")
        .map(resolve_base_url)
        .unwrap_or_default();

    let title = spec
        .get("info")
        .and_then(|i| i.get("title"))
        .and_then(|v| v.as_str())
        .unwrap_or(domain)
        .to_owned();

    let description = spec
        .get("info")
        .and_then(|i| i.get("description"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_owned();

    let version = spec
        .get("info")
        .and_then(|i| i.get("version"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_owned();

    let mut endpoints = Vec::new();
    let mut graphql_cache: HashMap<String, CatalogGraphqlSchema> = HashMap::new();

    if let Some(paths) = spec.get("paths").and_then(|v| v.as_object()) {
        for (path_str, path_item) in paths {
            let path_params: Vec<Value> = path_item
                .get("parameters")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();

            for method in HTTP_METHODS {
                if let Some(operation) = path_item.get(*method) {
                    endpoints.push(process_operation(
                        &spec,
                        spec_file,
                        method,
                        path_str,
                        operation,
                        &path_params,
                        common_types_dir,
                        &mut graphql_cache,
                    ));
                }
            }
        }
    }

    CatalogDomain {
        name: domain.to_owned(),
        title,
        description,
        version,
        base_url,
        endpoints,
    }
}
