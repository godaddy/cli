//! Regenerates `hosting-client/openapi/hosting.oas3.json` — progenitor's
//! codegen source — from the Hosting OpenAPI clone already pulled for the
//! API catalog.
//!
//! The published contract is OAS 3.1. Progenitor 0.14 consumes OAS 3.0, so
//! this pass dereferences `$ref`s, prefixes paths with `/v1/hosting` (the
//! CLI talks to the gateway host, not the spec's `/v1/hosting` server URL),
//! keeps 2xx responses, and relaxes component schemas so sparse or null
//! response fields deserialize instead of failing the typed client.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::{Map, Value};

const HOST: &str = "https://api.godaddy.com";

pub(crate) fn hosting_client_oas3_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../hosting-client/openapi/hosting.oas3.json")
}

pub(crate) fn refresh(
    spec_path: &Path,
    common_types: Option<&Path>,
    out_path: &Path,
) -> Result<()> {
    let (mut spec, defs) = crate::dereference::load_and_dereference(spec_path, common_types)
        .with_context(|| format!("failed to dereference Hosting spec {}", spec_path.display()))?;
    let object = spec
        .as_object_mut()
        .context("Hosting spec is not a JSON object")?;
    let schemas = object
        .entry("components")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .context("Hosting spec components is not an object")?
        .entry("schemas")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .context("Hosting spec components.schemas is not an object")?;
    for (name, definition) in defs {
        schemas.insert(name, definition);
    }
    crate::spec_normalize::rewrite_defs_refs(&mut spec);
    crate::spec_normalize::remove_remaining_relative_refs(&mut spec);
    crate::spec_normalize::remove_self_referencing_schemas(&mut spec);
    collapse_error_schema_alias(&mut spec);
    strip_codegen_validators(&mut spec);

    let object = spec
        .as_object_mut()
        .context("Hosting spec is not a JSON object")?;
    object.insert("openapi".to_owned(), Value::String("3.0.3".to_owned()));
    object.insert(
        "servers".to_owned(),
        serde_json::json!([{ "url": HOST, "description": "Hosting API host" }]),
    );
    crate::spec_normalize::prefix_paths(&mut spec, "/v1/hosting")?;
    prefer_json_source_import(&mut spec);
    drop_json_patch_operations(&mut spec);
    remove_protocol_header_parameters(&mut spec);
    crate::spec_normalize::retain_2xx_responses(&mut spec);
    strip_type_null_schemas(&mut spec);
    relax_response_schemas(&mut spec);
    prune_documentation_fields(&mut spec);
    crate::spec_normalize::add_required_response_descriptions(&mut spec);

    let json = serde_json::to_string_pretty(&spec).context("serialize Hosting codegen spec")?;
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    std::fs::write(out_path, json).with_context(|| format!("write {}", out_path.display()))?;
    eprintln!("   wrote {}", out_path.display());
    Ok(())
}

fn collapse_error_schema_alias(spec: &mut Value) {
    let error_is_ref = spec
        .pointer("/components/schemas/Error/$ref")
        .and_then(Value::as_str)
        == Some("#/components/schemas/error");
    if error_is_ref
        && let Some(schemas) = spec
            .pointer_mut("/components/schemas")
            .and_then(Value::as_object_mut)
    {
        schemas.remove("Error");
    }
    rewrite_error_refs(spec);
}

fn rewrite_error_refs(value: &mut Value) {
    match value {
        Value::Object(map) => {
            if map.get("$ref").and_then(Value::as_str) == Some("#/components/schemas/Error") {
                map.insert(
                    "$ref".to_owned(),
                    Value::String("#/components/schemas/error".to_owned()),
                );
            }
            for child in map.values_mut() {
                rewrite_error_refs(child);
            }
        }
        Value::Array(values) => {
            for value in values {
                rewrite_error_refs(value);
            }
        }
        _ => {}
    }
}

/// Drop string formats/patterns so progenitor emits `String` instead of
/// uuid/regex newtypes (`::uuid::Uuid`, `::regress::Regex`).
fn strip_codegen_validators(value: &mut Value) {
    match value {
        Value::Object(map) => {
            if map.get("type").and_then(Value::as_str) == Some("string")
                || map.contains_key("format")
                || map.contains_key("pattern")
            {
                map.remove("format");
                map.remove("pattern");
                map.remove("minLength");
                map.remove("maxLength");
            }
            for child in map.values_mut() {
                strip_codegen_validators(child);
            }
        }
        Value::Array(values) => {
            for value in values {
                strip_codegen_validators(value);
            }
        }
        _ => {}
    }
}

/// `POST /imports` is JSON (GitHub) or multipart (ZIP). Progenitor can only
/// emit one body type per operation; ZIP upload stays handwritten.
fn prefer_json_source_import(spec: &mut Value) {
    let Some(content) = spec
        .pointer_mut("/paths/~1v1~1hosting~1apps~1{appId}~1imports/post/requestBody/content")
        .and_then(Value::as_object_mut)
    else {
        return;
    };
    content.retain(|content_type, _| content_type == "application/json");
}

/// JSON Patch (RFC 6902) needs `application/json-patch+json`. Progenitor
/// serializes those bodies as ordinary JSON, so PATCH app/secrets stay on
/// the handwritten client.
fn drop_json_patch_operations(spec: &mut Value) {
    let Some(paths) = spec.pointer_mut("/paths").and_then(Value::as_object_mut) else {
        return;
    };
    for path in [
        "/v1/hosting/apps/{appId}",
        "/v1/hosting/apps/{appId}/secrets",
    ] {
        if let Some(item) = paths.get_mut(path).and_then(Value::as_object_mut) {
            item.remove("patch");
        }
    }
}

fn remove_protocol_header_parameters(spec: &mut Value) {
    let Some(paths) = spec.pointer_mut("/paths").and_then(Value::as_object_mut) else {
        return;
    };
    for item in paths.values_mut().filter_map(Value::as_object_mut) {
        for method in ["get", "post", "put", "delete", "patch"] {
            let Some(parameters) = item
                .get_mut(method)
                .and_then(|operation| operation.get_mut("parameters"))
                .and_then(Value::as_array_mut)
            else {
                continue;
            };
            parameters.retain(|parameter| {
                !matches!(
                    parameter.get("name").and_then(Value::as_str),
                    Some("traceparent") | Some("X-Request-Id") | Some("x-request-id")
                )
            });
        }
    }
}

/// OAS 3.1 encodes nullability as `{ "type": "null" }` in `anyOf`/`oneOf`.
/// Progenitor 0.14 panics on that (`not yet implemented: invalid type: null`).
fn strip_type_null_schemas(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for key in ["anyOf", "oneOf", "allOf"] {
                let Some(Value::Array(options)) = map.get(key) else {
                    continue;
                };
                let had_null = options
                    .iter()
                    .any(|option| option.get("type").and_then(Value::as_str) == Some("null"));
                if !had_null {
                    continue;
                }
                let mut options = options.clone();
                options.retain(|option| option.get("type").and_then(Value::as_str) != Some("null"));
                map.insert("nullable".to_owned(), Value::Bool(true));
                if options.len() == 1 {
                    map.remove(key);
                    if let Value::Object(only) = options.remove(0) {
                        for (k, v) in only {
                            map.entry(k).or_insert(v);
                        }
                    }
                } else {
                    map.insert(key.to_owned(), Value::Array(options));
                }
            }
            for child in map.values_mut() {
                strip_type_null_schemas(child);
            }
        }
        Value::Array(values) => {
            for value in values {
                strip_type_null_schemas(value);
            }
        }
        _ => {}
    }
}

fn relax_response_schemas(spec: &mut Value) {
    let Some(schemas) = spec
        .pointer_mut("/components/schemas")
        .and_then(Value::as_object_mut)
    else {
        return;
    };
    for schema in schemas.values_mut() {
        relax(schema);
    }
}

fn relax(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.remove("required");
            map.remove("format");
            map.remove("pattern");
            if map.get("$ref").and_then(Value::as_str) == Some("#") {
                map.clear();
            }
            if let Some(Value::Array(types)) = map.get("type") {
                if let Some(Value::String(value_type)) =
                    types.iter().find(|value| value.is_string())
                {
                    map.insert("type".to_owned(), Value::String(value_type.clone()));
                } else {
                    map.remove("type");
                }
            }
            if map.get("type").and_then(Value::as_str) == Some("array") {
                map.insert("nullable".to_owned(), Value::Bool(true));
            }
            for child in map.values_mut() {
                relax(child);
            }
        }
        Value::Array(values) => {
            for value in values {
                relax(value);
            }
        }
        _ => {}
    }
}

fn prune_documentation_fields(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.remove("description");
            map.remove("summary");
            map.remove("example");
            map.remove("externalDocs");
            map.remove("examples");
            for child in map.values_mut() {
                prune_documentation_fields(child);
            }
        }
        Value::Array(values) => {
            for value in values {
                prune_documentation_fields(value);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use crate::spec_normalize::prefix_paths;
    use serde_json::json;

    #[test]
    fn prefix_paths_nests_under_v1_hosting() {
        let mut spec = json!({
            "paths": {
                "/apps": { "get": {} },
                "not-a-path": { "get": {} }
            }
        });
        prefix_paths(&mut spec, "/v1/hosting").expect("prefix");
        assert!(spec["paths"].get("/v1/hosting/apps").is_some());
        assert!(spec["paths"].get("/apps").is_none());
        assert!(spec["paths"].get("not-a-path").is_none());
    }
}
