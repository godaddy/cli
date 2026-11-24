//! JSON-transform helpers shared by the progenitor-codegen spec modules
//! (`domains_spec`, `shopping_spec`, `email_spec`), which each turn a
//! `dereference::load_and_dereference`d catalog spec into a self-contained
//! OAS document `progenitor` can consume. Not used by the generic catalog
//! path (`dereference.rs`/`openapi.rs`) — its output isn't a real OpenAPI
//! document, so it never needs a `components.schemas` shaped this way.

use std::collections::HashSet;

use anyhow::{Context, Result};
use serde_json::Value;

/// Rewrites `#/$defs/<name>` pointers — left behind wherever
/// `dereference::load_and_dereference` lifted a multiply-referenced external
/// schema instead of inlining it — to `#/components/schemas/<name>`, the slot
/// `refresh()` re-inserts each lifted def into.
pub(crate) fn rewrite_defs_refs(value: &mut Value) {
    match value {
        Value::Object(map) => {
            if let Some(Value::String(reference)) = map.get("$ref")
                && let Some(name) = reference.strip_prefix("#/$defs/")
            {
                map.insert(
                    "$ref".to_owned(),
                    Value::String(format!("#/components/schemas/{name}")),
                );
            }
            for child in map.values_mut() {
                rewrite_defs_refs(child);
            }
        }
        Value::Array(values) => {
            for value in values {
                rewrite_defs_refs(value);
            }
        }
        _ => {}
    }
}

/// Removes any `$ref` that is still a relative file path after
/// dereferencing (neither a local `#/...` pointer nor an absolute URL) —
/// progenitor can't resolve it, and dereferencing should have already
/// inlined or lifted every reference that was reachable.
pub(crate) fn remove_remaining_relative_refs(value: &mut Value) {
    match value {
        Value::Object(map) => {
            if map
                .get("$ref")
                .and_then(Value::as_str)
                .is_some_and(|reference| !reference.starts_with('#') && !reference.contains("://"))
            {
                map.clear();
                return;
            }
            for child in map.values_mut() {
                remove_remaining_relative_refs(child);
            }
        }
        Value::Array(values) => {
            for value in values {
                remove_remaining_relative_refs(value);
            }
        }
        _ => {}
    }
}

/// Replaces any `components.schemas` entry that is a pure self-reference
/// (`{"$ref": "#/components/schemas/<its-own-name>"}`) with an empty schema.
/// Left as-is, progenitor would generate a struct wrapping itself — infinite
/// size.
pub(crate) fn remove_self_referencing_schemas(spec: &mut Value) {
    let Some(schemas) = spec
        .pointer_mut("/components/schemas")
        .and_then(Value::as_object_mut)
    else {
        return;
    };
    let names = schemas.keys().cloned().collect::<Vec<_>>();
    for name in names {
        let reference = format!("#/components/schemas/{name}");
        if schemas
            .get(&name)
            .and_then(|schema| schema.get("$ref"))
            .and_then(Value::as_str)
            == Some(reference.as_str())
        {
            schemas.insert(name, serde_json::json!({}));
        }
    }
}

/// Formats that map to external crates (`chrono`/`uuid`) progenitor doesn't
/// have available here — dropping the `format` keyword makes it fall back to
/// a bespoke newtype wrapping a plain `String` instead of `::chrono::DateTime`/
/// `::uuid::Uuid`.
const EXTERNAL_FORMATS: &[&str] = &["date", "date-time", "uuid", "partial-date-time"];

/// Drops string `format`s that map to external crates so the generated type
/// stays a plain `String`, and (unconditionally, on every string schema)
/// `pattern` — progenitor would otherwise emit a `regress`-backed validated
/// newtype, and `regress` isn't a dependency of any generated client crate
/// here. `minLength`/`maxLength` have no such hard requirement (progenitor
/// validates those with plain `std` string-length checks, no extra crate
/// needed), so they're only dropped when `drop_validators` is set — for a
/// spec where read schemas are known to drift outside their own declared
/// bounds on live responses (e.g. `domains_spec`'s v3 side), not as a
/// default.
pub(crate) fn strip_external_formats(value: &mut Value, drop_validators: bool) {
    match value {
        Value::Object(map) => {
            if map.get("type").and_then(Value::as_str) == Some("string") {
                if matches!(map.get("format").and_then(Value::as_str), Some(f) if EXTERNAL_FORMATS.contains(&f))
                {
                    map.remove("format");
                }
                map.remove("pattern");
                if drop_validators {
                    map.remove("minLength");
                    map.remove("maxLength");
                }
            }
            for child in map.values_mut() {
                strip_external_formats(child, drop_validators);
            }
        }
        Value::Array(values) => {
            for value in values {
                strip_external_formats(value, drop_validators);
            }
        }
        _ => {}
    }
}

/// Moves `prefix` from the spec's (often templated, e.g. `{environment}`)
/// `servers` entry onto every path. progenitor bakes each operation's
/// literal path into generated code, and the CLI constructs its client with
/// a bare host `base_url` (see e.g. `EmailClient`/`ShoppingClient`), so the
/// base path has to live in the path strings instead.
pub(crate) fn prefix_paths(spec: &mut Value, prefix: &str) -> Result<()> {
    let paths = spec
        .as_object_mut()
        .context("spec is not an object")?
        .remove("paths")
        .and_then(|paths| paths.as_object().cloned())
        .context("spec has no paths")?;
    let paths = paths
        .into_iter()
        .filter(|(path, _)| path.starts_with('/'))
        .map(|(path, item)| (format!("{prefix}{path}"), item))
        .collect();
    spec.as_object_mut()
        .expect("spec is an object (checked above)")
        .insert("paths".to_owned(), Value::Object(paths));
    Ok(())
}

/// Drops every non-2xx response from every operation. progenitor generates
/// one Rust variant per declared status; error responses across these
/// contracts are handled uniformly by the CLI's own HTTP-status error
/// mapping rather than a per-status generated type.
pub(crate) fn retain_2xx_responses(spec: &mut Value) {
    let Some(paths) = spec.pointer_mut("/paths").and_then(Value::as_object_mut) else {
        return;
    };
    for item in paths.values_mut().filter_map(Value::as_object_mut) {
        for method in ["get", "post", "put", "delete", "patch"] {
            let Some(responses) = item
                .get_mut(method)
                .and_then(|operation| operation.get_mut("responses"))
                .and_then(Value::as_object_mut)
            else {
                continue;
            };
            responses.retain(|status, _| status.starts_with('2'));
        }
    }
}

/// Ensures every response object has a `description` — required by the
/// OpenAPI spec but not always present in these contracts' hand-authored
/// docs, and progenitor's codegen expects it.
pub(crate) fn add_required_response_descriptions(value: &mut Value) {
    match value {
        Value::Object(map) => {
            if let Some(responses) = map.get_mut("responses").and_then(Value::as_object_mut) {
                for response in responses.values_mut().filter_map(Value::as_object_mut) {
                    response
                        .entry("description")
                        .or_insert_with(|| Value::String("Response".to_owned()));
                }
            }
            for child in map.values_mut() {
                add_required_response_descriptions(child);
            }
        }
        Value::Array(values) => {
            for value in values {
                add_required_response_descriptions(value);
            }
        }
        _ => {}
    }
}

/// Reduces a response's `content` map to just `application/json`, when
/// present — the API also advertises identical-schema xml/javascript
/// variants that would otherwise multiply progenitor's generated surface for
/// no benefit to a JSON-only client.
pub(crate) fn keep_only_json_content(op: &mut Value) {
    let Some(responses) = op.get_mut("responses").and_then(Value::as_object_mut) else {
        return;
    };
    for response in responses.values_mut() {
        let Some(content) = response.get_mut("content").and_then(Value::as_object_mut) else {
            continue;
        };
        if let Some(json_body) = content.remove("application/json") {
            content.clear();
            content.insert("application/json".to_owned(), json_body);
        }
    }
}

/// Collects the set of `#/components/schemas/<name>` targets referenced in
/// `value` (OAS3 side only — narrower than [`collect_ref_names`], which
/// would also match parameter/response refs).
pub(crate) fn collect_schema_ref_names(value: &Value) -> HashSet<String> {
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
pub(crate) fn rewrite_refs(value: &mut Value, rename: fn(&str, &str) -> String) {
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
pub(crate) fn prune_unreferenced_schemas(doc: &mut Value) {
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
    fn retain_2xx_responses_visits_every_method_and_skips_non_operations() {
        let mut spec = json!({ "paths": { "/stores": {
            "parameters": [{ "name": "x" }],
            "get": { "responses": { "200": {}, "404": {}, "default": {} } },
            "post": { "responses": { "201": {}, "400": {} } }
        } } });
        retain_2xx_responses(&mut spec);
        let item = &spec["paths"]["/stores"];
        let codes = |method: &str| -> Vec<String> {
            item[method]["responses"]
                .as_object()
                .expect("responses object")
                .keys()
                .cloned()
                .collect()
        };
        assert_eq!(codes("get"), ["200"]);
        assert_eq!(codes("post"), ["201"]);
        assert_eq!(item["parameters"][0]["name"], "x");
    }

    #[test]
    fn prefix_paths_prefixes_every_path_and_drops_path_level_extensions() {
        let mut spec = json!({ "paths": { "/stores": {}, "x-internal": {} } });
        prefix_paths(&mut spec, "/v2/commerce").expect("spec has paths");
        let paths = spec["paths"].as_object().expect("paths object");
        assert_eq!(paths.keys().collect::<Vec<_>>(), ["/v2/commerce/stores"]);
    }

    #[test]
    fn external_formats_and_patterns_are_stripped_but_other_formats_stay() {
        let mut schema = json!({ "properties": {
            "id": { "type": "string", "format": "uuid", "pattern": "^x$", "minLength": 1, "maxLength": 9 },
            "when": { "type": "string", "format": "date-time" },
            "email": { "type": "string", "format": "email" },
            "count": { "type": "integer", "format": "int64" }
        } });
        strip_external_formats(&mut schema, false);
        let props = &schema["properties"];
        assert_eq!(
            props["id"],
            json!({ "type": "string", "minLength": 1, "maxLength": 9 }),
            "length bounds stay unless validators are dropped"
        );
        assert_eq!(props["when"], json!({ "type": "string" }));
        assert_eq!(props["email"]["format"], "email");
        assert_eq!(props["count"]["format"], "int64");
    }

    #[test]
    fn dropping_validators_also_removes_length_bounds() {
        let mut schema = json!({ "type": "string", "minLength": 1, "maxLength": 9 });
        strip_external_formats(&mut schema, true);
        assert_eq!(schema, json!({ "type": "string" }));
    }
}
