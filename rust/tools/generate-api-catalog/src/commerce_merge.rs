//! Regenerates `commerce-client/openapi/commerce.oas3.json` — progenitor's
//! codegen source for the typed `commerce-client` crate — from the
//! already-cloned Stores OpenAPI spec `main.rs` also pulls for the catalog.

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use serde_json::{Map, Value};

use crate::spec_normalize;

/// Absolute path prefix the Stores API's operations live under, relative to
/// the bare API host `base_url` callers pass to the generated client at
/// runtime.
const BASE_PATH: &str = "/v2/commerce";

pub(crate) fn commerce_client_oas3_path() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir.join("../../commerce-client/openapi/commerce.oas3.json")
}

/// Regenerates `out_path` from the already-cloned `stores` spec clone. Set
/// `SKIP_COMMERCE_REFRESH` to leave `out_path` untouched (useful for local
/// iteration without re-deriving the transform).
pub(crate) fn refresh(
    stores_spec_path: &Path,
    common_types: Option<&Path>,
    out_path: &Path,
) -> Result<()> {
    if std::env::var("SKIP_COMMERCE_REFRESH").is_ok() {
        eprintln!(
            "SKIP_COMMERCE_REFRESH set — leaving {} untouched",
            out_path.display()
        );
        return Ok(());
    }
    eprintln!("Refreshing commerce-client codegen spec (stores)...");

    let stores = load_and_lift(stores_spec_path, common_types)
        .with_context(|| format!("failed to dereference {}", stores_spec_path.display()))?;
    ensure_refs_resolved(&stores)?;

    let json = serde_json::to_string_pretty(&stores).context("failed to serialize spec")?;
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    std::fs::write(out_path, json)
        .with_context(|| format!("failed to write {}", out_path.display()))?;
    eprintln!("   wrote {}", out_path.display());
    Ok(())
}

/// Fails if any `$ref` still points outside the document. The dereferencer
/// deliberately keeps a ref it cannot resolve instead of erroring, so without
/// this a moved or missing schema file would overwrite the last usable vendored
/// spec with one `progenitor` cannot resolve, before the catalog validation in
/// `main` has a chance to fail the run.
fn ensure_refs_resolved(spec: &Value) -> Result<()> {
    let unresolved = crate::dereference::collect_unresolved_refs(spec);
    anyhow::ensure!(
        unresolved.is_empty(),
        "Stores spec has unresolved external $refs: {unresolved:?}"
    );
    Ok(())
}

/// Dereferences `spec_path` (lifting its `$defs` into `components.schemas`
/// and rewriting `#/$defs/X` refs to `#/components/schemas/X`, since that's
/// the shape progenitor expects), pins `openapi` to `3.0.3` (progenitor
/// rejects anything else; this doc uses no 3.1-only constructs), prefixes
/// every path with [`BASE_PATH`], strips external formats, turns un-exploded
/// array query parameters into strings (see
/// [`flatten_unexploded_query_arrays`]), relaxes the response-only schemas to
/// tolerant readers (request-reachable schemas stay strict), and drops the
/// spec's *documented* non-2xx response schemas. That last step is a
/// codegen-input trim, not a runtime error-handling gap: every operation here
/// declares the same error schema, which `typify` would name `Error` and so
/// collide with `progenitor`'s own `Error<E>`, and real non-2xx responses are
/// still fully visible at request time via `commerce_client::Error::status()`.
fn load_and_lift(spec_path: &Path, common_types: Option<&Path>) -> Result<Value> {
    let (mut dereffed, defs) = crate::dereference::load_and_dereference(spec_path, common_types)?;

    let schemas = dereffed
        .as_object_mut()
        .context("spec is not a JSON object")?
        .entry("components")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .context("components is not a JSON object")?
        .entry("schemas")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .context("components.schemas is not a JSON object")?;
    for (key, def) in defs {
        schemas.insert(key, def);
    }
    spec_normalize::rewrite_defs_refs(&mut dereffed);

    dereffed
        .as_object_mut()
        .context("spec is not a JSON object")?
        .insert("openapi".to_owned(), Value::String("3.0.3".to_owned()));
    spec_normalize::prefix_paths(&mut dereffed, BASE_PATH)?;
    spec_normalize::retain_2xx_responses(&mut dereffed);
    // Every id and code field here is a plain string at the call sites, so
    // drop `minLength`/`maxLength` too, not just external formats and patterns.
    spec_normalize::strip_external_formats(&mut dereffed, true);
    flatten_unexploded_query_arrays(&mut dereffed);
    let strict = request_reachable_schemas(&dereffed);
    if let Some(schemas) = dereffed
        .pointer_mut("/components/schemas")
        .and_then(Value::as_object_mut)
    {
        for (name, schema) in schemas.iter_mut() {
            if !strict.contains(name) {
                relax(schema);
            }
        }
    }

    Ok(dereffed)
}

/// Names of the `components.schemas` entries a request body can reach,
/// directly or through nested `$ref`s. These stay strict so a required field
/// stays required at the call site (a body without `updateStoreAttribute`'s
/// `value` should not build). Schemas used by both requests and responses
/// stay strict too, the same trade the domains spec makes for its dual-use
/// schemas. Everything else is response-only and gets relaxed.
fn request_reachable_schemas(spec: &Value) -> HashSet<String> {
    let mut reachable = HashSet::new();
    let schemas = spec.pointer("/components/schemas");
    let Some(paths) = spec.get("paths").and_then(Value::as_object) else {
        return reachable;
    };
    for item in paths.values().filter_map(Value::as_object) {
        for operation in item.values() {
            if let Some(body) = operation.get("requestBody") {
                collect_schema_refs(body, schemas, &mut reachable);
            }
        }
    }
    reachable
}

/// Adds every `#/components/schemas/X` reachable from `node` to `seen`,
/// following refs into `schemas`. `seen` doubles as the visited set, so a
/// cyclic schema terminates.
fn collect_schema_refs(node: &Value, schemas: Option<&Value>, seen: &mut HashSet<String>) {
    match node {
        Value::Object(map) => {
            if let Some(Value::String(reference)) = map.get("$ref")
                && let Some(name) = reference.strip_prefix("#/components/schemas/")
                && seen.insert(name.to_owned())
                && let Some(target) = schemas.and_then(|all| all.get(name))
            {
                collect_schema_refs(target, schemas, seen);
            }
            for value in map.values() {
                collect_schema_refs(value, schemas, seen);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_schema_refs(item, schemas, seen);
            }
        }
        _ => {}
    }
}

/// Turns every array-typed query parameter declared `explode: false` into a
/// plain string. `progenitor` ignores `explode` and serializes an array as
/// repeated `key=value` pairs, so `includeFields=address&includeFields=attributes`
/// would go out where the API expects the single value
/// `includeFields=address,attributes` (the same trap `domain::common::comma_joined`
/// works around for the domains client). As a string, the generated setter
/// takes the already comma-joined value and sends exactly one pair.
fn flatten_unexploded_query_arrays(value: &mut Value) {
    match value {
        Value::Object(map) => {
            let is_unexploded_query_array = map.get("in").and_then(Value::as_str) == Some("query")
                && map.get("explode") == Some(&Value::Bool(false))
                && map
                    .get("schema")
                    .and_then(|schema| schema.get("type"))
                    .and_then(Value::as_str)
                    == Some("array");
            if is_unexploded_query_array {
                map.insert("schema".to_owned(), serde_json::json!({ "type": "string" }));
            }
            for child in map.values_mut() {
                flatten_unexploded_query_arrays(child);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(flatten_unexploded_query_arrays),
        _ => {}
    }
}

/// Drops `required` and marks array properties nullable so a present-but-null
/// value deserializes to `None` rather than erroring at *response*
/// deserialization time — it has no effect on what gets sent in a request.
/// Applied only to schemas no request body can reach (see
/// [`request_reachable_schemas`]), so a request keeps its required fields.
fn relax(defn: &mut Value) {
    let Some(map) = defn.as_object_mut() else {
        return;
    };
    map.remove("required");
    if let Some(properties) = map.get_mut("properties").and_then(Value::as_object_mut) {
        for pschema in properties.values_mut() {
            let is_array = pschema.get("type").and_then(Value::as_str) == Some("array");
            if is_array && let Some(pmap) = pschema.as_object_mut() {
                pmap.insert("nullable".to_owned(), Value::Bool(true));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A schema is strict if a request body reaches it directly or through
    /// nested refs, including through an array and through a cycle; a schema
    /// only a response reaches is not.
    #[test]
    fn request_reachable_schemas_follow_refs_and_ignore_response_only_ones() {
        let spec = json!({
            "paths": {
                "/stores/{id}/attributes/{name}": {
                    "put": {
                        "requestBody": { "content": { "application/json": {
                            "schema": { "$ref": "#/components/schemas/AttributeUpdate" }
                        } } },
                        "responses": { "200": { "content": { "application/json": {
                            "schema": { "$ref": "#/components/schemas/Store" }
                        } } } }
                    }
                },
                "/stores": {
                    "get": { "responses": { "200": { "content": { "application/json": {
                        "schema": { "$ref": "#/components/schemas/StoreList" }
                    } } } } }
                }
            },
            "components": { "schemas": {
                "AttributeUpdate": { "properties": {
                    "value": { "$ref": "#/components/schemas/Value" },
                    "tags": { "type": "array", "items": { "$ref": "#/components/schemas/Tag" } }
                } },
                "Value": { "properties": { "self": { "$ref": "#/components/schemas/Value" } } },
                "Tag": { "type": "object" },
                "Store": { "type": "object" },
                "StoreList": { "properties": { "items": { "$ref": "#/components/schemas/Store" } } }
            } }
        });

        let strict = request_reachable_schemas(&spec);

        for name in ["AttributeUpdate", "Value", "Tag"] {
            assert!(strict.contains(name), "{name} should be strict: {strict:?}");
        }
        for name in ["Store", "StoreList"] {
            assert!(
                !strict.contains(name),
                "{name} is response-only: {strict:?}"
            );
        }
    }

    #[test]
    fn an_unresolved_external_ref_stops_the_refresh() {
        let unresolved = json!({
            "paths": { "/stores": { "get": { "responses": { "200": {
                "content": { "application/json": {
                    "schema": { "$ref": "../common-types/store.yaml#/Store" }
                } }
            } } } } }
        });
        let error = ensure_refs_resolved(&unresolved).expect_err("an external ref is unresolved");
        assert!(
            error
                .to_string()
                .contains("../common-types/store.yaml#/Store"),
            "the error should name the ref: {error}"
        );

        let resolved = json!({
            "paths": { "/stores": { "get": { "responses": { "200": {
                "content": { "application/json": {
                    "schema": { "$ref": "#/components/schemas/Store" }
                } }
            } } } } }
        });
        assert!(ensure_refs_resolved(&resolved).is_ok());
    }

    #[test]
    fn only_unexploded_query_arrays_become_strings() {
        let mut spec = json!({
            "components": { "parameters": {
                "includeFields": {
                    "name": "includeFields", "in": "query", "explode": false, "style": "form",
                    "schema": { "type": "array", "items": { "type": "string" } }
                }
            } },
            "paths": { "/stores": { "get": { "parameters": [
                {
                    "name": "ids", "in": "query", "explode": true,
                    "schema": { "type": "array", "items": { "type": "string" } }
                },
                {
                    "name": "tags", "in": "query",
                    "schema": { "type": "array", "items": { "type": "string" } }
                },
                {
                    "name": "sort", "in": "query", "explode": false,
                    "schema": { "type": "string" }
                },
                {
                    "name": "fields", "in": "header", "explode": false,
                    "schema": { "type": "array", "items": { "type": "string" } }
                }
            ] } } }
        });

        flatten_unexploded_query_arrays(&mut spec);

        assert_eq!(
            spec["components"]["parameters"]["includeFields"]["schema"],
            json!({ "type": "string" })
        );
        let params = &spec["paths"]["/stores"]["get"]["parameters"];
        for (index, name) in [(0, "ids"), (1, "tags"), (3, "fields")] {
            assert_eq!(
                params[index]["schema"]["type"], "array",
                "{name} should stay an array"
            );
        }
        assert_eq!(params[2]["schema"], json!({ "type": "string" }));
    }

    #[test]
    fn a_spec_with_no_request_bodies_has_no_strict_schemas() {
        let spec = json!({
            "paths": { "/stores": { "get": { "responses": { "200": {} } } } },
            "components": { "schemas": { "Store": { "required": ["id"] } } }
        });
        assert!(request_reachable_schemas(&spec).is_empty());
    }

    #[test]
    fn relax_drops_required_and_makes_only_array_properties_nullable() {
        let mut schema = json!({
            "required": ["id"],
            "properties": {
                "tags": { "type": "array", "items": { "type": "string" } },
                "name": { "type": "string" }
            }
        });
        relax(&mut schema);
        assert!(schema.get("required").is_none());
        assert_eq!(schema["properties"]["tags"]["nullable"], true);
        assert!(schema["properties"]["name"].get("nullable").is_none());
    }
}
