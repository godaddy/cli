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

/// Path prefix of the v1 Stores API, served from the same host.
const V1_BASE_PATH: &str = "/v1/commerce";

/// The one v1 operation the client keeps: reading a store by id. v2 declares it
/// too, but the v2 route is not served everywhere yet (an unrouted path answers
/// with an empty 404), while v1 is. Every other v1 store operation is either
/// private upstream or already covered by v2, so nothing else is merged.
const V1_STORE_PATH: &str = "/stores/{storeId}";

/// The generated builder for the v1 operation reads `get_store_by_id_v1`, so it
/// cannot be mistaken for the v2 `get_store_by_id`.
const V1_STORE_OPERATION_ID: &str = "getStoreByIdV1";

/// Name prefix for every v1 schema, since v1 and v2 both define `Store`,
/// `Dictionary` and others with different shapes.
const V1_SCHEMA_PREFIX: &str = "V1";

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

    let mut stores = load_and_lift(stores_spec_path, common_types)
        .with_context(|| format!("failed to dereference {}", stores_spec_path.display()))?;
    merge_v1_get_store(&mut stores, stores_spec_path, common_types)
        .context("failed to merge the v1 getStoreById operation")?;
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

/// `<repo>/v1/schemas/openapi.yaml`, given the `<repo>/<version>/schemas/openapi.yaml`
/// the generator chose for the catalog. The pulled repository holds every
/// version side by side, so the v1 spec is already on disk and needs no fetch.
fn v1_spec_path(chosen: &Path) -> Result<PathBuf> {
    // .../<version>/schemas/openapi.yaml -> .../
    let repo_root = chosen
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .with_context(|| format!("cannot find the repository root from {}", chosen.display()))?;
    Ok(repo_root.join("v1").join("schemas").join("openapi.yaml"))
}

/// Adds the v1 `getStoreById` operation to `doc`, so one generated client serves
/// both API generations from one host, the way `domains-client` does.
///
/// v1 is kept to that one read, and its schemas are renamed with a [`V1_SCHEMA_PREFIX`]
/// prefix. `businessId` is declared when v1 requires it but never defines it (see
/// [`declare_required_business_id`]). On any failure `doc` is left unchanged.
fn merge_v1_get_store(
    doc: &mut Value,
    chosen_spec: &Path,
    common_types: Option<&Path>,
) -> Result<()> {
    let v1_path = v1_spec_path(chosen_spec)?;
    anyhow::ensure!(
        v1_path.exists(),
        "the v1 stores spec {} does not exist",
        v1_path.display()
    );
    let mut v1 = load_and_lift_v1(&v1_path, common_types)
        .with_context(|| format!("failed to read the v1 stores spec {}", v1_path.display()))?;

    let mut path_item = v1
        .get_mut("paths")
        .and_then(Value::as_object_mut)
        .and_then(|paths| paths.remove(V1_STORE_PATH))
        .with_context(|| format!("the v1 stores spec has no {V1_STORE_PATH}"))?;
    let mut operation = path_item
        .as_object_mut()
        .and_then(|item| item.remove("get"))
        .with_context(|| format!("the v1 stores spec has no GET {V1_STORE_PATH}"))?;
    operation["operationId"] = Value::String(V1_STORE_OPERATION_ID.to_owned());
    spec_normalize::keep_only_json_content(&mut operation);

    // The path item's own parameters (store id, request id, field selection)
    // apply to the operation, so they travel with it. Dropping the item's
    // other methods drops the private PATCH.
    let mut kept = serde_json::Map::new();
    if let Some(parameters) = path_item.get("parameters") {
        kept.insert("parameters".to_owned(), parameters.clone());
    }
    kept.insert("get".to_owned(), operation);
    let kept = Value::Object(kept);

    // Keep only the schemas the kept operation reaches. Pruning is done on a
    // v1-only document, so it can never remove anything from the v2 one.
    let schemas = v1
        .pointer("/components/schemas")
        .cloned()
        .unwrap_or_else(|| Value::Object(serde_json::Map::new()));
    let mut v1_only = serde_json::json!({
        "paths": { V1_STORE_PATH: kept },
        "components": { "schemas": schemas },
    });
    spec_normalize::prune_unreferenced_schemas(&mut v1_only);
    let mut kept = v1_only["paths"][V1_STORE_PATH].take();
    let mut schemas = v1_only["components"]["schemas"].take();
    spec_normalize::rewrite_refs(&mut kept, rename_v1);
    spec_normalize::rewrite_refs(&mut schemas, rename_v1);
    // Only schemas are merged, so a reference into any other component section
    // (a shared response, parameter or header) would dangle. v1's reads inline
    // theirs today; if that changes, say so instead of writing a broken document.
    ensure_only_schema_refs(&kept)?;
    ensure_only_schema_refs(&schemas)?;

    let v1_path_key = format!("{V1_BASE_PATH}{V1_STORE_PATH}");
    let mut merged = doc.clone();
    let paths = merged
        .pointer_mut("/paths")
        .and_then(Value::as_object_mut)
        .context("the stores document has no paths")?;
    anyhow::ensure!(
        !paths.contains_key(&v1_path_key),
        "{v1_path_key} is already in the document"
    );
    paths.insert(v1_path_key, kept);

    let target = merged
        .pointer_mut("/components/schemas")
        .and_then(Value::as_object_mut)
        .context("the stores document has no components.schemas")?;
    for (name, body) in schemas.as_object().into_iter().flatten() {
        let renamed = format!("{V1_SCHEMA_PREFIX}{name}");
        anyhow::ensure!(
            !target.contains_key(&renamed),
            "components.schemas.{renamed} is already in the document"
        );
        target.insert(renamed, body.clone());
    }

    // Before relaxing: `relax` drops the `required` list this reads.
    declare_required_business_id(&mut merged);
    relax_v1_schemas(&mut merged);

    // A reference to a schema the document does not define would stop
    // `progenitor`. `ensure_refs_resolved` only sees references that leave the
    // document, so check the local ones here, while `doc` can still be left alone.
    let missing = unresolved_local_schema_refs(&merged);
    anyhow::ensure!(
        missing.is_empty(),
        "the merged stores document references schemas it does not define: {missing:?}"
    );
    *doc = merged;
    Ok(())
}

/// Names that a `#/components/schemas/<name>` reference points at but that
/// `components.schemas` does not define, sorted and without duplicates.
fn unresolved_local_schema_refs(doc: &Value) -> Vec<String> {
    let defined: HashSet<&str> = doc
        .pointer("/components/schemas")
        .and_then(Value::as_object)
        .map(|schemas| schemas.keys().map(String::as_str).collect())
        .unwrap_or_default();
    let mut missing: Vec<String> = spec_normalize::collect_schema_ref_names(doc)
        .into_iter()
        .filter(|name| !defined.contains(name.as_str()))
        .collect();
    missing.sort();
    missing
}

/// Removes every `default` from the schemas under `components.schemas`.
/// `progenitor` turns a property default into a value it fills in when the field
/// is absent, so a store the API sent without a `status` would read as `ACTIVE`,
/// indistinguishable from one that was. Without the default the field is an
/// `Option` and the CLI shows only what the API said. The hosting spec does the
/// same for the one default it has.
///
/// Walks schemas, not data: the keys of a `properties` map are property names
/// (one may be called `default`), and an `enum`, `const` or example holds values,
/// so none of those are touched.
fn strip_property_defaults(doc: &mut Value) {
    fn strip(schema: &mut Value) {
        match schema {
            Value::Object(map) => {
                map.remove("default");
                for (key, child) in map.iter_mut() {
                    match key.as_str() {
                        "properties" | "patternProperties" | "$defs" | "definitions" => {
                            for sub_schema in
                                child.as_object_mut().into_iter().flatten().map(|(_, v)| v)
                            {
                                strip(sub_schema);
                            }
                        }
                        "enum" | "const" | "example" | "examples" => {}
                        _ => strip(child),
                    }
                }
            }
            Value::Array(items) => items.iter_mut().for_each(strip),
            _ => {}
        }
    }
    if let Some(schemas) = doc
        .pointer_mut("/components/schemas")
        .and_then(Value::as_object_mut)
    {
        schemas.values_mut().for_each(strip);
    }
}

/// Fails if `value` holds a `$ref` into a `components` section other than
/// `schemas`, which the merge does not carry over.
fn ensure_only_schema_refs(value: &Value) -> Result<()> {
    match value {
        Value::Object(map) => {
            if let Some(Value::String(reference)) = map.get("$ref")
                && reference.starts_with("#/components/")
                && !reference.starts_with("#/components/schemas/")
            {
                anyhow::bail!(
                    "the v1 operation references {reference}, but only schemas are merged"
                );
            }
            map.values().try_for_each(ensure_only_schema_refs)
        }
        Value::Array(items) => items.iter().try_for_each(ensure_only_schema_refs),
        _ => Ok(()),
    }
}

/// Relaxes every v1 schema to a tolerant reader. All of them are response-only,
/// since the only v1 operation kept is a read, so none needs to stay strict.
fn relax_v1_schemas(doc: &mut Value) {
    if let Some(schemas) = doc
        .pointer_mut("/components/schemas")
        .and_then(Value::as_object_mut)
    {
        for (name, schema) in schemas.iter_mut() {
            if name.starts_with(V1_SCHEMA_PREFIX) {
                relax(schema);
            }
        }
    }
}

fn rename_v1(_section: &str, name: &str) -> String {
    format!("{V1_SCHEMA_PREFIX}{name}")
}

/// The v1 spec lists `businessId` as required on a store but never declares the
/// property (v2 declares it), while a live v1 record does carry it. Without a
/// declaration the generated v1 store would have no `business_id` at all, so
/// declare it, as the string id it is, when it is required and missing.
fn declare_required_business_id(doc: &mut Value) {
    let Some(store) = doc.pointer_mut(&format!("/components/schemas/{V1_SCHEMA_PREFIX}Store"))
    else {
        return;
    };
    let requires = store
        .get("required")
        .and_then(Value::as_array)
        .is_some_and(|required| required.iter().any(|name| name == "businessId"));
    if !requires {
        return;
    }
    if let Some(properties) = store.get_mut("properties").and_then(Value::as_object_mut) {
        properties
            .entry("businessId")
            .or_insert_with(|| serde_json::json!({ "type": "string" }));
    }
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
    let mut dereffed = dereference_and_lift_defs(spec_path, common_types)?;

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
    strip_property_defaults(&mut dereffed);

    Ok(dereffed)
}

/// Dereferences `spec_path` and lifts the schemas it pulled in from other files
/// into `components.schemas`, rewriting `#/$defs/X` refs to
/// `#/components/schemas/X`: the shape `progenitor` expects. Shared by the v2
/// document and the v1 operation merged into it.
fn dereference_and_lift_defs(spec_path: &Path, common_types: Option<&Path>) -> Result<Value> {
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
    Ok(dereffed)
}

/// The v1 spec, prepared the way the v2 one is for the parts that apply to a
/// kept read: lifted schemas, only 2xx responses, no external formats or length
/// bounds, and un-exploded array query parameters (v1's `includeField`) turned
/// into strings. Paths are not prefixed here; the merge places the one operation
/// it keeps.
fn load_and_lift_v1(spec_path: &Path, common_types: Option<&Path>) -> Result<Value> {
    let mut v1 = dereference_and_lift_defs(spec_path, common_types)?;
    spec_normalize::retain_2xx_responses(&mut v1);
    spec_normalize::strip_external_formats(&mut v1, true);
    flatten_unexploded_query_arrays(&mut v1);
    strip_property_defaults(&mut v1);
    Ok(v1)
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
mod tests;
