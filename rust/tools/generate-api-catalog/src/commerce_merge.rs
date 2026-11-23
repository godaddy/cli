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
    *doc = merged;
    Ok(())
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
mod tests {
    use super::*;
    use serde_json::json;

    /// A v1 stores spec in the layout the pulled repo has (`v1/schemas/openapi.yaml`
    /// beside `v2/schemas/openapi.yaml`), with the operation the client keeps, the
    /// private and unrelated operations it must not, and a schema that nothing the
    /// kept operation reaches uses.
    const V1_FIXTURE: &str = r##"
openapi: 3.0.3
info: { title: Stores v1, version: "1.0" }
paths:
  /stores/{storeId}:
    parameters:
      - { name: storeId, in: path, required: true, schema: { type: string } }
      - name: includeField
        in: query
        explode: false
        style: form
        schema: { type: array, items: { type: string } }
    get:
      operationId: getStoreById
      responses:
        "200":
          description: Store Found
          content:
            application/json:
              schema: { $ref: "#/components/schemas/Store" }
            application/xml:
              schema: { $ref: "#/components/schemas/Store" }
        "404": { description: missing }
    patch:
      operationId: updateStore
      x-visibility: { extent: private }
      responses: { "200": { description: ok } }
  /businesses/{businessId}/stores:
    get:
      operationId: getStoresByBusinessId
      responses:
        "200":
          description: ok
          content:
            application/json:
              schema: { $ref: "#/components/schemas/Links" }
components:
  schemas:
    Store:
      type: object
      required: [displayName, businessId]
      properties:
        displayName: { type: string }
        processor: { $ref: "#/components/schemas/Processor" }
    Processor:
      type: object
      properties:
        name: { type: string }
    Links:
      type: object
      properties:
        next: { type: string }
"##;

    /// Writes `<root>/v1/schemas/openapi.yaml` and returns the path of a sibling
    /// `<root>/v2/schemas/openapi.yaml`, which is what the generator hands in.
    fn repo_with_v1(root: &std::path::Path, v1: &str) -> std::path::PathBuf {
        let v1_dir = root.join("v1").join("schemas");
        std::fs::create_dir_all(&v1_dir).expect("create v1 dir");
        std::fs::write(v1_dir.join("openapi.yaml"), v1).expect("write v1 spec");
        let v2_dir = root.join("v2").join("schemas");
        std::fs::create_dir_all(&v2_dir).expect("create v2 dir");
        v2_dir.join("openapi.yaml")
    }

    fn v2_doc() -> Value {
        json!({
            "openapi": "3.0.3",
            "paths": { "/v2/commerce/stores/{storeId}": { "get": { "operationId": "getStoreById" } } },
            "components": { "schemas": { "Store": { "type": "object" } } }
        })
    }

    fn merged_v1() -> Value {
        let dir = tempfile::tempdir().expect("tempdir");
        let spec = repo_with_v1(dir.path(), V1_FIXTURE);
        let mut doc = v2_doc();
        merge_v1_get_store(&mut doc, &spec, None).expect("merges");
        doc
    }

    #[test]
    fn only_the_v1_get_store_operation_is_merged_under_its_own_name_and_path() {
        let doc = merged_v1();
        let paths = doc["paths"].as_object().expect("paths object");
        let mut keys: Vec<&str> = paths.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "/v1/commerce/stores/{storeId}",
                "/v2/commerce/stores/{storeId}"
            ],
            "a private v1 operation or an unrelated one leaked in"
        );
        let item = &doc["paths"]["/v1/commerce/stores/{storeId}"];
        assert_eq!(item["get"]["operationId"], "getStoreByIdV1");
        assert!(
            item.get("patch").is_none(),
            "the private PATCH must not be merged"
        );
        assert_eq!(
            doc["paths"]["/v2/commerce/stores/{storeId}"]["get"]["operationId"], "getStoreById",
            "the v2 operation must be untouched"
        );
    }

    #[test]
    fn the_v1_operation_keeps_its_parameters_and_only_its_json_response() {
        let doc = merged_v1();
        let item = &doc["paths"]["/v1/commerce/stores/{storeId}"];
        let names: Vec<&str> = item["parameters"]
            .as_array()
            .expect("path-level parameters are kept")
            .iter()
            .filter_map(|p| p["name"].as_str())
            .collect();
        assert_eq!(names, ["storeId", "includeField"]);
        let content = item["get"]["responses"]["200"]["content"]
            .as_object()
            .expect("content object");
        assert_eq!(content.keys().collect::<Vec<_>>(), ["application/json"]);
    }

    #[test]
    fn v1_schemas_are_prefixed_so_they_cannot_collide_with_v2s_and_every_ref_follows() {
        let doc = merged_v1();
        let schemas = doc["components"]["schemas"].as_object().expect("schemas");
        assert!(
            schemas.contains_key("V1Store"),
            "{:?}",
            schemas.keys().collect::<Vec<_>>()
        );
        assert!(
            schemas.contains_key("V1Processor"),
            "reachable from the store"
        );
        assert_eq!(
            schemas["Store"],
            json!({ "type": "object" }),
            "the v2 Store must be untouched"
        );
        assert_eq!(
            doc["paths"]["/v1/commerce/stores/{storeId}"]["get"]["responses"]["200"]["content"]["application/json"]
                ["schema"]["$ref"],
            "#/components/schemas/V1Store"
        );
        assert_eq!(
            schemas["V1Store"]["properties"]["processor"]["$ref"],
            "#/components/schemas/V1Processor"
        );
    }

    #[test]
    fn schemas_the_kept_operation_does_not_reach_are_not_merged() {
        let doc = merged_v1();
        assert!(
            doc["components"]["schemas"].get("V1Links").is_none(),
            "V1Links is only used by an operation that is not kept"
        );
    }

    /// The v1 spec lists `businessId` as required on a store but never declares
    /// the property, while a live v1 record does carry it. Without declaring it the
    /// generated v1 store would have no `business_id`.
    #[test]
    fn a_required_but_undeclared_business_id_is_declared() {
        let doc = merged_v1();
        assert_eq!(
            doc["components"]["schemas"]["V1Store"]["properties"]["businessId"]["type"],
            "string"
        );
    }

    #[test]
    fn a_business_id_the_spec_already_declares_is_left_as_it_is() {
        let fixed = V1_FIXTURE.replace(
            "        displayName: { type: string }\n",
            "        displayName: { type: string }\n        businessId: { type: string, description: from upstream }\n",
        );
        let dir = tempfile::tempdir().expect("tempdir");
        let spec = repo_with_v1(dir.path(), &fixed);
        let mut doc = v2_doc();
        merge_v1_get_store(&mut doc, &spec, None).expect("merges");
        assert_eq!(
            doc["components"]["schemas"]["V1Store"]["properties"]["businessId"]["description"],
            "from upstream"
        );
    }

    #[test]
    fn the_v1_spec_is_found_next_to_the_version_directory_the_generator_chose() {
        let path = std::path::Path::new(
            "/tmp/clone/commerce.stores-specification/v2/schemas/openapi.yaml",
        );
        assert_eq!(
            v1_spec_path(path).expect("derives"),
            std::path::PathBuf::from(
                "/tmp/clone/commerce.stores-specification/v1/schemas/openapi.yaml"
            )
        );
        assert!(v1_spec_path(std::path::Path::new("openapi.yaml")).is_err());
    }

    #[test]
    fn a_missing_v1_spec_is_an_error_naming_it_not_a_silent_skip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let v2_dir = dir.path().join("v2").join("schemas");
        std::fs::create_dir_all(&v2_dir).expect("create dir");
        let mut doc = v2_doc();
        let error = merge_v1_get_store(&mut doc, &v2_dir.join("openapi.yaml"), None)
            .expect_err("no v1 spec is an error");
        assert!(format!("{error:#}").contains("v1"), "{error:#}");
        assert_eq!(
            doc,
            v2_doc(),
            "a failed merge must leave the document alone"
        );
    }

    /// Merging v1 must not change v2: a v2 schema that no v2 path references is
    /// still there afterwards (the fixture's v2 `Store` is exactly that).
    #[test]
    fn the_v2_document_is_left_exactly_as_it_was() {
        let doc = merged_v1();
        let before = v2_doc();
        assert_eq!(
            doc["paths"]["/v2/commerce/stores/{storeId}"],
            before["paths"]["/v2/commerce/stores/{storeId}"]
        );
        assert_eq!(
            doc["components"]["schemas"]["Store"],
            before["components"]["schemas"]["Store"]
        );
    }

    #[test]
    fn only_schemas_are_merged_not_v1s_other_component_sections() {
        let with_sections = V1_FIXTURE.replace(
            "components:\n  schemas:",
            "components:\n  parameters:\n    requestId: { name: Poynt-Request-Id, in: header, schema: { type: string } }\n  headers:\n    Location: { schema: { type: string } }\n  schemas:",
        );
        let dir = tempfile::tempdir().expect("tempdir");
        let spec = repo_with_v1(dir.path(), &with_sections);
        let mut doc = v2_doc();
        merge_v1_get_store(&mut doc, &spec, None).expect("merges");
        let components = doc["components"].as_object().expect("components");
        assert_eq!(
            components.keys().collect::<Vec<_>>(),
            ["schemas"],
            "another v1 component section leaked in"
        );
    }

    /// The dereferencer already inlines a local ref into a non-schema component
    /// section (a shared header, say), so the v1 operation arrives self-contained
    /// and nothing dangles even though only schemas are merged.
    #[test]
    fn a_shared_header_is_inlined_and_does_not_leave_a_dangling_ref() {
        let shared = V1_FIXTURE
            .replace(
                "          description: Store Found\n",
                "          description: Store Found\n          headers:\n            Location: { $ref: \"#/components/headers/Location\" }\n",
            )
            .replace(
                "components:\n  schemas:",
                "components:\n  headers:\n    Location: { schema: { type: string } }\n  schemas:",
            );
        let dir = tempfile::tempdir().expect("tempdir");
        let spec = repo_with_v1(dir.path(), &shared);
        let mut doc = v2_doc();
        merge_v1_get_store(&mut doc, &spec, None).expect("merges");
        let response = &doc["paths"]["/v1/commerce/stores/{storeId}"]["get"]["responses"]["200"];
        assert_eq!(response["headers"]["Location"]["schema"]["type"], "string");
        assert!(
            !doc.to_string().contains("#/components/headers"),
            "a ref into a section that is not merged was left behind"
        );
    }

    /// Backstop for the case above: if a ref into another component section ever
    /// does survive, merging only schemas would leave it dangling, so say so.
    #[test]
    fn a_reference_into_an_unmerged_component_section_is_reported() {
        let dangling = json!({ "get": { "responses": { "200": {
            "headers": { "Location": { "$ref": "#/components/headers/Location" } }
        } } } });
        let error = ensure_only_schema_refs(&dangling).expect_err("would dangle");
        assert!(
            error.to_string().contains("#/components/headers/Location"),
            "{error}"
        );
        let fine = json!({ "schema": { "$ref": "#/components/schemas/Store" } });
        assert!(ensure_only_schema_refs(&fine).is_ok());
    }

    /// v1 is only ever read, so its schemas are tolerant readers: no `required`
    /// list to fail a sparse record on, and an array property may be `null`.
    #[test]
    fn v1_schemas_are_relaxed_readers() {
        let with_array = V1_FIXTURE.replace(
            "        displayName: { type: string }\n",
            "        displayName: { type: string }\n        governmentIds: { type: array, items: { type: string } }\n",
        );
        let dir = tempfile::tempdir().expect("tempdir");
        let spec = repo_with_v1(dir.path(), &with_array);
        let mut doc = v2_doc();
        merge_v1_get_store(&mut doc, &spec, None).expect("merges");
        let store = &doc["components"]["schemas"]["V1Store"];
        assert!(
            store.get("required").is_none(),
            "a v1 schema is still strict: {store}"
        );
        assert_eq!(store["properties"]["governmentIds"]["nullable"], true);
    }

    #[test]
    fn a_v1_spec_without_the_operation_is_an_error() {
        let without = V1_FIXTURE.replace("/stores/{storeId}:", "/stores/{id}:");
        let dir = tempfile::tempdir().expect("tempdir");
        let spec = repo_with_v1(dir.path(), &without);
        let mut doc = v2_doc();
        let error = merge_v1_get_store(&mut doc, &spec, None).expect_err("operation is gone");
        assert!(
            format!("{error:#}").contains("/stores/{storeId}"),
            "{error:#}"
        );
    }

    #[test]
    fn merging_over_an_existing_v1_path_or_schema_is_an_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let spec = repo_with_v1(dir.path(), V1_FIXTURE);

        let mut path_taken = v2_doc();
        path_taken["paths"]["/v1/commerce/stores/{storeId}"] = json!({});
        assert!(merge_v1_get_store(&mut path_taken, &spec, None).is_err());

        let mut schema_taken = v2_doc();
        schema_taken["components"]["schemas"]["V1Store"] = json!({});
        assert!(merge_v1_get_store(&mut schema_taken, &spec, None).is_err());
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
