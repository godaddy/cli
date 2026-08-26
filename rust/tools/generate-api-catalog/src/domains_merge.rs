//! Regenerates `domains-client/openapi/domains.oas3.json` — progenitor's
//! codegen source for the typed `domains-client` crate — from the same v3
//! spec clone `main.rs` already pulled for the catalog, merged with the one
//! v1 operation v3 doesn't yet serve (`GET /v1/domains/agreements`).
//!
//! Ported from the former `domains-client/scripts/{trim,merge}-spec.py` so
//! this and the embedded API catalog come from a single `cargo run -p
//! generate-api-catalog` invocation instead of a separately-run pipeline.
//! The transformations below are deliberately faithful to those scripts —
//! see their historical git history for the reasoning behind each one.

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, bail};
use serde_json::{Map, Value};

const V1_SWAGGER_URL: &str = "https://developer.godaddy.com/swagger/swagger_domains.json";
const HOST: &str = "https://api.godaddy.com";
const SWAGGER2OPENAPI_VERSION: &str = "swagger2openapi@7.0.8";
const USER_AGENT: &str = "godaddy-cli-api-catalog-generator";

const EXTERNAL_FORMATS: &[&str] = &["date", "date-time", "uuid", "partial-date-time"];

/// v3 schemas the CLI *constructs* (request bodies) or that are dual-use
/// (request + response): keep them strict so required fields stay required at
/// the call site. Everything else in v3 `components.schemas` is a pure read
/// and gets relaxed (required dropped, arrays made nullable) to tolerate
/// sparse payloads.
const STRICT_V3: &[&str] = &[
    "AvailabilityCheckCriteria",
    "Registration",
    "Consent",
    "ConsentActor",
    "Contact",
    "Contacts",
    "simple-address",
    "InlineRegistrationProfile",
    "DNSRecord",
    "NameServers",
    "NameserverHostname",
];

/// The only v1 operation v3 does not yet serve: legal agreements for a TLD.
/// Upstream `operationId` is `getAgreement`; pinned here to `agreements` so
/// the generated builder reads `client.agreements()`.
const V1_RETAINED_OPS: &[(&str, &str)] = &[("/v1/domains/agreements", "agreements")];

/// Regenerates `out_path` from `v3_spec_path` (the already-cloned
/// `domains.domain-lifecycle-specification` v3 OpenAPI doc) merged with a
/// freshly-pulled, trimmed copy of the v1 Swagger spec. Set
/// `SKIP_DOMAINS_REFRESH` to leave `out_path` untouched (useful for local
/// iteration without a Node/network dependency).
pub(crate) fn refresh(v3_spec_path: &Path, out_path: &Path) -> Result<()> {
    if std::env::var("SKIP_DOMAINS_REFRESH").is_ok() {
        eprintln!(
            "SKIP_DOMAINS_REFRESH set — leaving {} untouched",
            out_path.display()
        );
        return Ok(());
    }
    eprintln!("Refreshing domains-client codegen spec (v1 + v3 merge)...");

    // The v1 `agreements` operation has no v3 equivalent yet (v3's quote flow
    // only echoes agreement *types* the caller already accepted, not the
    // lookup-by-TLD `domain agreements` needs), so a v1 fetch failure must
    // not silently drop it from the generated client. Warn and leave
    // `out_path` untouched rather than failing the whole catalog run over an
    // upstream v1 doc outage — the rest of the catalog pull is unaffected.
    let v1 = match fetch_and_convert_v1() {
        Ok(v1) => v1,
        Err(err) => {
            eprintln!(
                "WARNING: failed to refresh the v1 domains spec ({err:#}) — \
                 leaving {} untouched",
                out_path.display()
            );
            return Ok(());
        }
    };

    let v3_text = std::fs::read_to_string(v3_spec_path)
        .with_context(|| format!("failed to read v3 spec at {}", v3_spec_path.display()))?;
    let mut v3: Value = serde_yaml::from_str(&v3_text)
        .with_context(|| format!("failed to parse v3 spec {}", v3_spec_path.display()))?;

    merge(&mut v3, v1)?;

    let json = serde_json::to_string_pretty(&v3).context("failed to serialize merged spec")?;
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    std::fs::write(out_path, json)
        .with_context(|| format!("failed to write {}", out_path.display()))?;
    eprintln!("   wrote {}", out_path.display());
    Ok(())
}

// ---------------------------------------------------------------------------
// v1: fetch, trim, Swagger2 -> OpenAPI3
// ---------------------------------------------------------------------------

fn fetch_and_convert_v1() -> Result<Value> {
    let client = reqwest::blocking::Client::builder()
        .user_agent(USER_AGENT)
        .build()
        .context("failed to build http client")?;
    let swagger_v2: Value = client
        .get(V1_SWAGGER_URL)
        .send()
        .context("failed to fetch v1 swagger spec")?
        .error_for_status()
        .context("v1 swagger spec fetch returned an error status")?
        .json()
        .context("failed to parse v1 swagger JSON")?;

    let trimmed = trim_v1(&swagger_v2)?;

    let in_file = tempfile::NamedTempFile::new().context("failed to create temp file")?;
    std::fs::write(in_file.path(), serde_json::to_vec(&trimmed)?)
        .context("failed to write trimmed v1 spec to temp file")?;
    let out_file = tempfile::NamedTempFile::new().context("failed to create temp file")?;

    let status = Command::new("npx")
        .args(["-y", SWAGGER2OPENAPI_VERSION])
        .arg(path_str(in_file.path())?)
        .args(["-o", path_str(out_file.path())?])
        .status()
        .context("failed to run npx swagger2openapi — is Node/npx installed?")?;
    if !status.success() {
        bail!("swagger2openapi conversion failed with status {status}");
    }

    let oas3_text = std::fs::read_to_string(out_file.path())
        .context("failed to read converted v1 OAS3 document")?;
    serde_json::from_str(&oas3_text).context("failed to parse converted v1 OAS3 document")
}

fn path_str(path: &Path) -> Result<&str> {
    path.to_str()
        .with_context(|| format!("non-UTF8 temp path {}", path.display()))
}

/// Trims the upstream Swagger 2.0 spec to the retained v1 subset
/// (`GET /v1/domains/agreements`) plus the transitive closure of definitions
/// it references, relaxing every retained definition to a tolerant reader.
fn trim_v1(swagger: &Value) -> Result<Value> {
    let paths_in = swagger
        .get("paths")
        .and_then(Value::as_object)
        .context("v1 swagger spec missing paths")?;
    let definitions_in = swagger
        .get("definitions")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();

    let mut paths_out = Map::new();
    for (path, op_id) in V1_RETAINED_OPS {
        let mut get = paths_in
            .get(*path)
            .and_then(|p| p.get("get"))
            .with_context(|| format!("v1 swagger spec missing GET {path}"))?
            .clone();
        get["operationId"] = Value::String((*op_id).to_owned());
        get["produces"] = Value::Array(vec![Value::String("application/json".to_owned())]);
        filter_2xx_responses(&mut get);
        let mut path_obj = Map::new();
        path_obj.insert("get".to_owned(), get);
        paths_out.insert((*path).to_owned(), Value::Object(path_obj));
    }
    let mut paths_value = Value::Object(paths_out);
    dedup_enums(&mut paths_value);

    let mut needed: HashSet<String> = HashSet::new();
    let mut stack: Vec<String> = collect_ref_names(&paths_value).into_iter().collect();
    while let Some(name) = stack.pop() {
        if !needed.insert(name.clone()) {
            continue;
        }
        if let Some(def) = definitions_in.get(&name) {
            for r in collect_ref_names(def) {
                if !needed.contains(&r) {
                    stack.push(r);
                }
            }
        }
    }

    let mut needed_sorted: Vec<&String> = needed.iter().collect();
    needed_sorted.sort();
    let mut definitions_out = Map::new();
    for name in needed_sorted {
        if let Some(def) = definitions_in.get(name) {
            let mut def = def.clone();
            relax(&mut def, "x-nullable");
            definitions_out.insert(name.clone(), def);
        }
    }

    let mut out = serde_json::json!({
        "swagger": "2.0",
        "info": {
            "title": "GoDaddy Domains API (retained v1 subset: agreements)",
            "version": swagger.get("info").and_then(|i| i.get("version")).and_then(Value::as_str).unwrap_or("1.0.0"),
        },
        "host": swagger.get("host").and_then(Value::as_str).unwrap_or("api.ote-godaddy.com"),
        "basePath": swagger.get("basePath").and_then(Value::as_str).unwrap_or("/"),
        "schemes": swagger.get("schemes").cloned().unwrap_or_else(|| serde_json::json!(["https"])),
        "paths": paths_value,
        "definitions": definitions_out,
    });
    strip_external_formats(&mut out, false);
    Ok(out)
}

// ---------------------------------------------------------------------------
// v3 + v1(OAS3) merge
// ---------------------------------------------------------------------------

/// Merges `v1` into `v3` in place, producing the single OAS3 document
/// progenitor's `build.rs` consumes.
///
/// The CLI's `domain`/`dns` commands talk to two API generations at once: v3
/// for availability/suggest/get/list/quote/register and the full DNS record
/// lifecycle, and v1 for the one operation v3 does not yet serve
/// (agreements). One progenitor client over one host base URL requires
/// merging both into a single document:
///
///   * v3 is the base. Its paths are rewritten to the absolute
///     `/v3/domains/...` form and `servers` is pinned to the bare host, so one
///     `base_url` serves both `/v3/domains/...` and `/v1/domains/...`.
///   * v1 is injected with a `V1` name prefix — v1 and v3 both define
///     `DNSRecord`, `DomainStatus`, ... with different shapes.
fn merge(v3: &mut Value, mut v1: Value) -> Result<()> {
    let v3_obj = v3.as_object_mut().context("v3 spec is not a JSON object")?;

    let old_paths = v3_obj
        .remove("paths")
        .and_then(|p| p.as_object().cloned())
        .context("v3 spec missing paths")?;
    let mut new_paths = Map::new();
    for (key, item) in old_paths {
        if !key.starts_with('/') {
            continue; // drop paths-level extensions (e.g. x-visibility)
        }
        new_paths.insert(format!("/v3/domains{key}"), item);
    }
    v3_obj.insert("paths".to_owned(), Value::Object(new_paths));
    v3_obj.insert(
        "servers".to_owned(),
        serde_json::json!([{ "url": HOST, "description": "Domains API host" }]),
    );

    only_2xx_paths(v3_obj.get_mut("paths").context("missing paths")?);
    strip_external_formats(v3, true);

    if let Some(schemas) = v3
        .pointer_mut("/components/schemas")
        .and_then(Value::as_object_mut)
    {
        let names: Vec<String> = schemas.keys().cloned().collect();
        for name in names {
            if STRICT_V3.contains(&name.as_str()) {
                continue;
            }
            if let Some(schema) = schemas.get_mut(&name) {
                relax(schema, "nullable");
            }
        }
    }

    // `Registration` is dual-use: the register request requires `quoteToken`,
    // but the register/get *responses* never echo it. progenitor emits one
    // Rust type for both directions, so keeping `quoteToken` required makes
    // response deserialization fail. Mark just that field optional.
    if let Some(required) = v3
        .pointer_mut("/components/schemas/Registration/required")
        .and_then(Value::as_array_mut)
    {
        required.retain(|f| f.as_str() != Some("quoteToken"));
    }

    // v1: prefix every component name with V1, rewrite its $refs, then merge.
    let v1_obj = v1.as_object_mut().context("v1 spec is not a JSON object")?;
    let mut v1_paths = v1_obj.remove("paths").unwrap_or(Value::Object(Map::new()));
    let v1_components = v1_obj
        .remove("components")
        .and_then(|c| c.as_object().cloned())
        .unwrap_or_default();

    rewrite_refs(&mut v1_paths, rename_v1);
    let mut v1_components = v1_components;
    for section in v1_components.values_mut() {
        if section.is_object() {
            rewrite_refs(section, rename_v1);
        }
    }

    let v3_components = v3
        .as_object_mut()
        .context("v3 spec is not a JSON object")?
        .entry("components")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .context("v3 components is not a JSON object")?;
    for (section, members) in v1_components {
        let Value::Object(members) = members else {
            continue;
        };
        let dst = v3_components
            .entry(section)
            .or_insert_with(|| Value::Object(Map::new()))
            .as_object_mut()
            .context("v3 component section is not a JSON object")?;
        for (name, body) in members {
            dst.insert(format!("V1{name}"), body);
        }
    }

    only_2xx_paths(&mut v1_paths);
    if let (Value::Object(v3_paths), Value::Object(v1_paths)) =
        (v3.pointer_mut("/paths").context("missing paths")?, v1_paths)
    {
        v3_paths.extend(v1_paths);
    }

    prune_unreferenced_schemas(v3);
    Ok(())
}

fn rename_v1(_section: &str, name: &str) -> String {
    format!("V1{name}")
}

// ---------------------------------------------------------------------------
// Shared JSON-tree helpers (mirrors the former trim/merge Python scripts)
// ---------------------------------------------------------------------------

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

/// Applies [`filter_2xx_responses`] to every HTTP-method operation under each
/// path item.
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
/// generated types stay plain `String`. When `drop_validators` is set, also
/// drops `pattern`/`minLength`/`maxLength` so progenitor doesn't emit a
/// validated newtype — used on the v3 side only, matching the former
/// `merge-spec.py` behavior (the v1 side only ever dropped `format`).
fn strip_external_formats(value: &mut Value, drop_validators: bool) {
    match value {
        Value::Object(map) => {
            if map.get("type").and_then(Value::as_str) == Some("string") {
                if matches!(map.get("format").and_then(Value::as_str), Some(f) if EXTERNAL_FORMATS.contains(&f))
                {
                    map.remove("format");
                }
                if drop_validators {
                    map.remove("pattern");
                    map.remove("minLength");
                    map.remove("maxLength");
                }
            }
            for v in map.values_mut() {
                strip_external_formats(v, drop_validators);
            }
        }
        Value::Array(items) => {
            for v in items {
                strip_external_formats(v, drop_validators);
            }
        }
        _ => {}
    }
}

/// Drops `required` and marks array properties nullable so a present-but-null
/// value deserializes to `None` rather than erroring. `nullable_key` is
/// `"x-nullable"` for the Swagger 2.0 side or `"nullable"` for OAS3.
fn relax(defn: &mut Value, nullable_key: &str) {
    let Some(map) = defn.as_object_mut() else {
        return;
    };
    map.remove("required");
    if let Some(properties) = map.get_mut("properties").and_then(Value::as_object_mut) {
        for pschema in properties.values_mut() {
            let is_array = pschema.get("type").and_then(Value::as_str) == Some("array");
            if is_array && let Some(pmap) = pschema.as_object_mut() {
                pmap.insert(nullable_key.to_owned(), Value::Bool(true));
            }
        }
    }
}

/// typify/progenitor can't build a Rust enum from case-only duplicates (e.g.
/// `["FAST","FULL","fast","full"]`); keeps the first occurrence per
/// case-fold (non-string enum values dedup on their canonical JSON text).
fn dedup_enums(value: &mut Value) {
    match value {
        Value::Object(map) => {
            if let Some(Value::Array(items)) = map.get_mut("enum") {
                let mut seen = HashSet::new();
                let mut uniq = Vec::with_capacity(items.len());
                for item in items.drain(..) {
                    let key = match &item {
                        Value::String(s) => s.to_lowercase(),
                        other => other.to_string(),
                    };
                    if seen.insert(key) {
                        uniq.push(item);
                    }
                }
                *items = uniq;
            }
            for v in map.values_mut() {
                dedup_enums(v);
            }
        }
        Value::Array(items) => {
            for v in items {
                dedup_enums(v);
            }
        }
        _ => {}
    }
}

/// Collects every `$ref` target's final path segment, regardless of which
/// section it points into (used for the v1 Swagger 2.0 `#/definitions/X`
/// closure walk, which has only one section).
fn collect_ref_names(value: &Value) -> HashSet<String> {
    let mut out = HashSet::new();
    collect_ref_names_into(value, &mut out);
    out
}

fn collect_ref_names_into(value: &Value, out: &mut HashSet<String>) {
    match value {
        Value::Object(map) => {
            if let Some(Value::String(r)) = map.get("$ref")
                && let Some(name) = r.rsplit('/').next()
            {
                out.insert(name.to_owned());
            }
            for v in map.values() {
                collect_ref_names_into(v, out);
            }
        }
        Value::Array(items) => {
            for v in items {
                collect_ref_names_into(v, out);
            }
        }
        _ => {}
    }
}

/// Collects the set of `#/components/schemas/<name>` targets referenced in
/// `value` (OAS3 side only — narrower than [`collect_ref_names`], which
/// would also match parameter/response refs).
fn collect_schema_ref_names(value: &Value) -> HashSet<String> {
    let mut out = HashSet::new();
    collect_schema_ref_names_into(value, &mut out);
    out
}

fn collect_schema_ref_names_into(value: &Value, out: &mut HashSet<String>) {
    match value {
        Value::Object(map) => {
            if let Some(Value::String(r)) = map.get("$ref")
                && let Some(name) = r.strip_prefix("#/components/schemas/")
            {
                out.insert(name.to_owned());
            }
            for v in map.values() {
                collect_schema_ref_names_into(v, out);
            }
        }
        Value::Array(items) => {
            for v in items {
                collect_schema_ref_names_into(v, out);
            }
        }
        _ => {}
    }
}

/// Rewrites every `#/components/<section>/<name>` `$ref` via `rename`.
fn rewrite_refs(value: &mut Value, rename: fn(&str, &str) -> String) {
    match value {
        Value::Object(map) => {
            if let Some(Value::String(r)) = map.get("$ref") {
                let parts: Vec<&str> = r.split('/').collect();
                if parts.len() == 4 && parts[0] == "#" && parts[1] == "components" {
                    let renamed = rename(parts[2], parts[3]);
                    map.insert(
                        "$ref".to_owned(),
                        Value::String(format!("#/components/{}/{renamed}", parts[2])),
                    );
                }
            }
            for v in map.values_mut() {
                rewrite_refs(v, rename);
            }
        }
        Value::Array(items) => {
            for v in items {
                rewrite_refs(v, rename);
            }
        }
        _ => {}
    }
}

/// Drops component schemas unreachable from the (2xx-stripped) paths and the
/// other component sections. After stripping error responses this removes
/// the orphaned error-envelope model whose sanitized Rust name (`Error`)
/// would otherwise collide with the common `error` schema.
fn prune_unreferenced_schemas(doc: &mut Value) {
    let Some(schemas) = doc
        .pointer("/components/schemas")
        .and_then(Value::as_object)
        .cloned()
    else {
        return;
    };

    let mut seed = doc
        .get("paths")
        .map(collect_schema_ref_names)
        .unwrap_or_default();
    if let Some(components) = doc.pointer("/components").and_then(Value::as_object) {
        for (section, members) in components {
            if section != "schemas" {
                seed.extend(collect_schema_ref_names(members));
            }
        }
    }

    let mut reachable: HashSet<String> = HashSet::new();
    let mut stack: Vec<String> = seed.into_iter().collect();
    while let Some(name) = stack.pop() {
        if !reachable.insert(name.clone()) {
            continue;
        }
        if let Some(schema) = schemas.get(&name) {
            stack.extend(collect_schema_ref_names(schema));
        }
    }

    if let Some(schemas) = doc
        .pointer_mut("/components/schemas")
        .and_then(Value::as_object_mut)
    {
        let to_remove: Vec<String> = schemas
            .keys()
            .filter(|k| !reachable.contains(k.as_str()))
            .cloned()
            .collect();
        for name in to_remove {
            schemas.remove(&name);
        }
    }
}

/// Resolves the on-disk clone path progenitor's build reads.
pub(crate) fn domains_client_oas3_path() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir.join("../../domains-client/openapi/domains.oas3.json")
}
