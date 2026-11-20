//! Regenerates `commerce-client/openapi/commerce.oas3.json` — progenitor's
//! codegen source for the typed `commerce-client` crate — from the
//! already-cloned Stores OpenAPI spec `main.rs` also pulls for the catalog.

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use serde_json::{Map, Value};

/// Absolute path prefix the Stores API's operations live under, relative to
/// the bare API host `base_url` callers pass to the generated client at
/// runtime.
const BASE_PATH: &str = "/v2/commerce";

/// Drops string `format`s that map to external crates (chrono/uuid) so the
/// generated types stay plain `String` — every id in this crate is handled
/// as a plain string at the call sites already. Mirrors
/// `domains_merge.rs::EXTERNAL_FORMATS`.
const EXTERNAL_FORMATS: &[&str] = &["date", "date-time", "uuid", "partial-date-time"];

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

/// Dereferences `spec_path` (lifting its `$defs` into `components.schemas`
/// and rewriting `#/$defs/X` refs to `#/components/schemas/X`, since that's
/// the shape progenitor expects), pins `openapi` to `3.0.3` (progenitor
/// rejects anything else; this doc uses no 3.1-only constructs), prefixes
/// every path with [`BASE_PATH`], strips external formats, relaxes the
/// response-only schemas to tolerant readers (request-reachable schemas stay
/// strict), and drops the spec's *documented* non-2xx
/// response schemas (see [`only_2xx_paths`] — this is a codegen-input trim,
/// not a runtime error-handling gap: real non-2xx responses are still fully
/// handled at request time via `commerce_client::Error::status()`).
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
    rewrite_defs_refs(&mut dereffed);

    let obj = dereffed
        .as_object_mut()
        .context("spec is not a JSON object")?;
    obj.insert("openapi".to_owned(), Value::String("3.0.3".to_owned()));

    let old_paths = obj
        .remove("paths")
        .and_then(|p| p.as_object().cloned())
        .context("spec missing paths")?;
    let mut new_paths = Map::new();
    for (key, item) in old_paths {
        if !key.starts_with('/') {
            continue; // drop paths-level extensions
        }
        new_paths.insert(format!("{BASE_PATH}{key}"), item);
    }
    obj.insert("paths".to_owned(), Value::Object(new_paths));

    only_2xx_paths(obj.get_mut("paths").context("missing paths")?);
    strip_external_formats(&mut dereffed);
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

fn rewrite_defs_refs(value: &mut Value) {
    match value {
        Value::Object(map) => {
            if let Some(Value::String(r)) = map.get("$ref")
                && let Some(rest) = r.strip_prefix("#/$defs/")
            {
                map.insert(
                    "$ref".to_owned(),
                    Value::String(format!("#/components/schemas/{rest}")),
                );
            }
            for v in map.values_mut() {
                rewrite_defs_refs(v);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(rewrite_defs_refs),
        _ => {}
    }
}

/// Drops `op`'s declared 4xx/5xx/`default` response entries — i.e. removes
/// them from the OpenAPI *document* `progenitor` reads, not from what the
/// generated client can actually see at request time. `progenitor` still
/// reports a real non-2xx response as `Error::UnexpectedResponse` (with the
/// true status and body), so callers can still map it to a proper
/// `NOT_FOUND`/`AUTH_REQUIRED`/etc. error — this function only changes what
/// *type* progenitor generates for the error case, not whether errors are
/// caught.
///
/// The reason to drop them: every operation here declares the *same* error
/// schema (`#/$defs/error`). Left in, `typify` would try to generate a Rust
/// type for it named `Error` (schema names get PascalCased) — colliding
/// with `progenitor`'s own top-level `Error<E>` enum, a hard compile error.
/// Stripping these responses makes every operation's error type resolve to
/// `()` instead (see `progenitor_impl::method::extract_responses`'s
/// fallback to `OperationResponseKind::None` when no error response is
/// declared), which is also why one error-mapping helper over `Error<()>` can
/// handle every method uniformly rather than needing per-operation typed
/// error handling.
fn filter_2xx_responses(op: &mut Value) {
    if let Some(responses) = op.get_mut("responses").and_then(Value::as_object_mut) {
        let codes: Vec<String> = responses.keys().cloned().collect();
        for code in codes {
            if !code.starts_with('2') {
                responses.remove(&code);
            }
        }
    }
}

/// Applies [`filter_2xx_responses`] to every operation under every path.
fn only_2xx_paths(paths: &mut Value) {
    const METHODS: &[&str] = &["get", "post", "put", "patch", "delete"];
    let Some(paths) = paths.as_object_mut() else {
        return;
    };
    for item in paths.values_mut() {
        let Some(item) = item.as_object_mut() else {
            continue;
        };
        for method in METHODS {
            if let Some(op) = item.get_mut(*method) {
                filter_2xx_responses(op);
            }
        }
    }
}

/// Drops string `format`s that map to external crates (chrono/uuid) so the
/// generated types stay plain `String`, and drops `pattern`/`minLength`/
/// `maxLength` so `typify` doesn't emit a `regress`-validated newtype for
/// every id/code field (this crate's callers always have a plain `String`
/// already validated by the CLI's own argument parsing, not a `progenitor`-
/// generated wrapper type).
fn strip_external_formats(value: &mut Value) {
    match value {
        Value::Object(map) => {
            if map.get("type").and_then(Value::as_str) == Some("string") {
                if matches!(map.get("format").and_then(Value::as_str), Some(f) if EXTERNAL_FORMATS.contains(&f))
                {
                    map.remove("format");
                }
                map.remove("pattern");
                map.remove("minLength");
                map.remove("maxLength");
            }
            for v in map.values_mut() {
                strip_external_formats(v);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(strip_external_formats),
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

    #[test]
    fn defs_refs_are_rewritten_to_components_schemas_at_any_depth() {
        let mut spec = json!({
            "a": { "$ref": "#/$defs/Store" },
            "b": [{ "items": { "$ref": "#/$defs/Address" } }],
            "c": { "$ref": "#/components/schemas/Already" },
        });
        rewrite_defs_refs(&mut spec);
        assert_eq!(spec["a"]["$ref"], "#/components/schemas/Store");
        assert_eq!(
            spec["b"][0]["items"]["$ref"],
            "#/components/schemas/Address"
        );
        assert_eq!(spec["c"]["$ref"], "#/components/schemas/Already");
    }

    #[test]
    fn only_2xx_responses_survive() {
        let mut op = json!({
            "responses": {
                "200": { "description": "ok" },
                "201": { "description": "created" },
                "404": { "description": "missing" },
                "default": { "description": "error" }
            }
        });
        filter_2xx_responses(&mut op);
        let codes: Vec<&str> = op["responses"]
            .as_object()
            .expect("responses object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(codes, ["200", "201"]);
    }

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
    fn a_spec_with_no_request_bodies_has_no_strict_schemas() {
        let spec = json!({
            "paths": { "/stores": { "get": { "responses": { "200": {} } } } },
            "components": { "schemas": { "Store": { "required": ["id"] } } }
        });
        assert!(request_reachable_schemas(&spec).is_empty());
    }

    #[test]
    fn only_2xx_paths_visits_every_method_but_skips_non_operations() {
        let mut paths = json!({
            "/stores": {
                "parameters": [{ "name": "x" }],
                "get": { "responses": { "200": {}, "500": {} } },
                "post": { "responses": { "201": {}, "400": {} } }
            }
        });
        only_2xx_paths(&mut paths);
        assert!(paths["/stores"]["get"]["responses"].get("500").is_none());
        assert!(paths["/stores"]["post"]["responses"].get("400").is_none());
        assert_eq!(paths["/stores"]["parameters"][0]["name"], "x");
    }

    #[test]
    fn external_formats_and_string_constraints_are_stripped_but_other_formats_stay() {
        let mut schema = json!({
            "properties": {
                "id": { "type": "string", "format": "uuid", "pattern": "^x$", "minLength": 1, "maxLength": 9 },
                "when": { "type": "string", "format": "date-time" },
                "email": { "type": "string", "format": "email" },
                "count": { "type": "integer", "format": "int64" }
            }
        });
        strip_external_formats(&mut schema);
        let props = &schema["properties"];
        assert_eq!(props["id"], json!({ "type": "string" }));
        assert_eq!(props["when"], json!({ "type": "string" }));
        assert_eq!(props["email"]["format"], "email");
        assert_eq!(props["count"]["format"], "int64");
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
