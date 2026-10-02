//! JSON-transform helpers shared by the progenitor-codegen spec modules
//! (`domains_spec`, `shopping_spec`, `email_spec`), which each turn a
//! `dereference::load_and_dereference`d catalog spec into a self-contained
//! OAS document `progenitor` can consume. Not used by the generic catalog
//! path (`dereference.rs`/`openapi.rs`) — its output isn't a real OpenAPI
//! document, so it never needs a `components.schemas` shaped this way.

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
