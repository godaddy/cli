//! Pre-filter for spec nodes marked private.
//!
//! Upstream specs can carry `x-visibility: { extent: private }` at any level
//! (an operation, a schema, a property, a tag, ...) to say the node is not part
//! of the public API surface. Everything the CLI derives from a spec — the
//! catalog and the progenitor-codegen documents — must leave those nodes out,
//! so [`prune_private`] strips them from each document as it is loaded, before
//! any `$ref` is resolved. A private node is therefore never lifted into
//! `$defs`, and a `$ref` that points at one is left unresolved. (A `$ref`'d file
//! whose own root is private can't be pruned from inside, so the loader rejects
//! it outright.)

use std::collections::HashSet;

use indexmap::IndexMap;
use std::path::Path;

use anyhow::{Result, bail};
use serde_json::Value;

use crate::openapi::HTTP_METHODS;

pub(crate) fn is_private(value: &Value) -> bool {
    value
        .get("x-visibility")
        .and_then(|visibility| visibility.get("extent"))
        .and_then(Value::as_str)
        == Some("private")
}

/// Removes every object entry and array element marked
/// `x-visibility.extent: private` from `value`, at any depth. A pruned
/// `properties` entry is also dropped from its schema's `required` list so the
/// schema stays valid. The top-level value itself is never removed.
pub(crate) fn prune_private(value: &mut Value) {
    match value {
        Value::Object(map) => {
            let pruned_properties = map
                .get_mut("properties")
                .and_then(Value::as_object_mut)
                .map(|properties| {
                    let private: Vec<String> = properties
                        .iter()
                        .filter(|(_, property)| is_private(property))
                        .map(|(name, _)| name.clone())
                        .collect();
                    for name in &private {
                        properties.remove(name);
                    }
                    private
                })
                .unwrap_or_default();
            if !pruned_properties.is_empty()
                && let Some(required) = map.get_mut("required").and_then(Value::as_array_mut)
            {
                required.retain(|name| {
                    name.as_str()
                        .is_none_or(|name| !pruned_properties.iter().any(|pruned| pruned == name))
                });
            }

            let private_keys: Vec<String> = map
                .iter()
                .filter(|(_, child)| is_private(child))
                .map(|(key, _)| key.clone())
                .collect();
            for key in &private_keys {
                map.remove(key);
            }
            for child in map.values_mut() {
                prune_private(child);
            }
        }
        Value::Array(values) => {
            values.retain(|child| !is_private(child));
            for child in values {
                prune_private(child);
            }
        }
        _ => {}
    }
}

/// Fails if the primary spec document is itself marked private: pruning only
/// removes marked children, so a private root would otherwise export all of its
/// unmarked operations. Drop such a spec from the source manifest instead.
pub(crate) fn ensure_public_root(spec_file: &Path, spec: &Value) -> Result<()> {
    if is_private(spec) {
        bail!(
            "{} is marked private (x-visibility) and can't be synced",
            spec_file.display()
        );
    }
    Ok(())
}

/// JSON pointers (`#/a/b`) of every node [`prune_private`] would remove from
/// `root`. Collected before pruning so [`ensure_no_refs_to_private`] can tell a
/// reference broken by the filter from one that was already dangling upstream.
pub(crate) fn private_pointers(root: &Value) -> Vec<String> {
    let mut out = Vec::new();
    collect_private_pointers(root, "#", &mut out);
    out
}

fn collect_private_pointers(value: &Value, pointer: &str, out: &mut Vec<String>) {
    let children: Vec<(String, &Value)> = match value {
        Value::Object(map) => map
            .iter()
            .map(|(key, child)| (key.replace('~', "~0").replace('/', "~1"), child))
            .collect(),
        Value::Array(values) => values
            .iter()
            .enumerate()
            .map(|(index, child)| (index.to_string(), child))
            .collect(),
        _ => return,
    };
    for (segment, child) in children {
        let child_pointer = format!("{pointer}/{segment}");
        if is_private(child) {
            out.push(child_pointer);
        } else {
            collect_private_pointers(child, &child_pointer, out);
        }
    }
}

/// Fails if a dereferenced spec still holds a local `$ref` into a node the
/// filter removed (see [`private_pointers`]). Dereferencing leaves a `$ref` it
/// can't resolve in place, and `collect_unresolved_refs` ignores local
/// pointers, so without this a public node pointing at a private component
/// would sync "successfully" with a reference the API explorer can't follow.
pub(crate) fn ensure_no_refs_to_private(
    spec_file: &Path,
    private: &[String],
    spec: &Value,
    defs: &IndexMap<String, Value>,
) -> Result<()> {
    let mut found = Vec::new();
    collect_refs_to_private(spec, private, &mut found);
    for def in defs.values() {
        collect_refs_to_private(def, private, &mut found);
    }
    found.sort();
    found.dedup();
    if !found.is_empty() {
        bail!(
            "{} $ref(s) in {} point at nodes marked private:\n  {}",
            found.len(),
            spec_file.display(),
            found.join("\n  ")
        );
    }
    Ok(())
}

fn collect_refs_to_private(value: &Value, private: &[String], out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            if let Some(Value::String(reference)) = map.get("$ref")
                && private.iter().any(|pointer| {
                    reference == pointer
                        || reference
                            .strip_prefix(pointer.as_str())
                            .is_some_and(|rest| rest.starts_with('/'))
                })
            {
                out.push(reference.clone());
            }
            for child in map.values() {
                collect_refs_to_private(child, private, out);
            }
        }
        Value::Array(values) => {
            for child in values {
                collect_refs_to_private(child, private, out);
            }
        }
        _ => {}
    }
}

/// Drops path items left with no operations. Run on a dereferenced spec after
/// [`prune_private`], so a path whose every operation was private doesn't
/// linger as an empty (or parameters-only) entry. Non-path `paths` keys
/// (`x-*` extensions) are left alone.
pub(crate) fn drop_operationless_paths(spec: &mut Value) {
    let Some(paths) = spec.get_mut("paths").and_then(Value::as_object_mut) else {
        return;
    };
    paths.retain(|path, item| {
        !path.starts_with('/')
            || item.as_object().is_none_or(|item| {
                // An unresolved `$ref` item is left for the caller's reference
                // validation to report, not silently dropped.
                item.contains_key("$ref")
                    || HTTP_METHODS.iter().any(|method| item.contains_key(*method))
            })
    });
}

/// Drops the lifted `$defs` that nothing in `catalog` reaches, directly or
/// through other defs. Pruning private operations strands the schemas only
/// they used; without this they stay in the catalog and `gddy api schema get`
/// would still return them. Any `#/$defs/<name>` string counts as a reference,
/// so discriminator mappings (plain strings, not `$ref`s) keep their targets.
/// Returns the number of defs dropped.
pub(crate) fn retain_reachable_defs(catalog: &Value, defs: &mut IndexMap<String, Value>) -> usize {
    let mut reachable: HashSet<String> = HashSet::new();
    let mut stack: Vec<String> = Vec::new();
    collect_defs_refs(catalog, &mut stack);
    while let Some(name) = stack.pop() {
        if !reachable.insert(name.clone()) {
            continue;
        }
        if let Some(def) = defs.get(&name) {
            collect_defs_refs(def, &mut stack);
        }
    }
    let before = defs.len();
    defs.retain(|name, _| reachable.contains(name));
    before - defs.len()
}

fn collect_defs_refs(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::String(text) => {
            if let Some(rest) = text.strip_prefix("#/$defs/") {
                let key = rest.split('/').next().unwrap_or(rest);
                out.push(key.replace("~1", "/").replace("~0", "~"));
            }
        }
        Value::Object(map) => {
            for child in map.values() {
                collect_defs_refs(child, out);
            }
        }
        Value::Array(values) => {
            for child in values {
                collect_defs_refs(child, out);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn prunes_private_operations_and_emptied_paths() {
        let mut spec = json!({
            "paths": {
                "/open": {
                    "get": { "operationId": "open" },
                    "post": { "operationId": "hidden", "x-visibility": { "extent": "private" } }
                },
                "/secret": {
                    "parameters": [{ "name": "id", "in": "query" }],
                    "get": { "operationId": "secret", "x-visibility": { "extent": "private" } }
                }
            }
        });
        prune_private(&mut spec);
        drop_operationless_paths(&mut spec);
        assert_eq!(
            spec,
            json!({ "paths": { "/open": { "get": { "operationId": "open" } } } })
        );
    }

    #[test]
    fn prunes_array_elements_and_nested_nodes() {
        let mut spec = json!({
            "tags": [
                { "name": "public" },
                { "name": "internal", "x-visibility": { "extent": "private" } }
            ],
            "components": { "schemas": {
                "Hidden": { "type": "object", "x-visibility": { "extent": "private" } },
                "Shown": { "type": "object" }
            } }
        });
        prune_private(&mut spec);
        assert_eq!(
            spec,
            json!({
                "tags": [{ "name": "public" }],
                "components": { "schemas": { "Shown": { "type": "object" } } }
            })
        );
    }

    #[test]
    fn drops_private_property_from_required() {
        let mut spec = json!({
            "type": "object",
            "required": ["id", "debug"],
            "properties": {
                "id": { "type": "string" },
                "debug": { "type": "string", "x-visibility": { "extent": "private" } }
            }
        });
        prune_private(&mut spec);
        assert_eq!(
            spec,
            json!({
                "type": "object",
                "required": ["id"],
                "properties": { "id": { "type": "string" } }
            })
        );
    }

    #[test]
    fn keeps_public_and_unrecognized_visibility() {
        let original = json!({
            "x-visibility": { "extent": "public" },
            "info": {
                "x-visibility": { "extent": "public" },
                "contact": { "x-visibility": "private" }
            },
            "paths": { "/a": { "get": { "x-visibility": { "extent": "internal" } } } }
        });
        let mut spec = original.clone();
        prune_private(&mut spec);
        drop_operationless_paths(&mut spec);
        assert_eq!(spec, original);
    }

    #[test]
    fn keeps_non_path_extensions_in_paths() {
        let mut spec = json!({ "paths": { "x-note": {}, "/a": {} } });
        drop_operationless_paths(&mut spec);
        assert_eq!(spec, json!({ "paths": { "x-note": {} } }));
    }

    #[test]
    fn load_and_dereference_filters_root_and_external_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("Widget.yaml"),
            "type: object\nrequired: [id, debug]\nproperties:\n  id:\n    type: string\n  debug:\n    type: string\n    x-visibility:\n      extent: private\n",
        )
        .expect("write model");
        let spec_path = dir.path().join("openapi.yaml");
        std::fs::write(
            &spec_path,
            "openapi: '3.0.0'\npaths:\n  /widgets:\n    get:\n      responses:\n        '200':\n          content:\n            application/json:\n              schema:\n                $ref: './Widget.yaml'\n    post:\n      x-visibility:\n        extent: private\n  /internal:\n    get:\n      x-visibility:\n        extent: private\n",
        )
        .expect("write spec");

        let (spec, defs) =
            crate::dereference::load_and_dereference(&spec_path, None).expect("dereference");

        let paths = spec["paths"].as_object().expect("paths object");
        assert_eq!(paths.keys().collect::<Vec<_>>(), ["/widgets"]);
        assert!(paths["/widgets"].get("post").is_none());
        assert_eq!(
            defs["Widget"]["required"],
            json!(["id"]),
            "private property must be dropped from the lifted schema's required list"
        );
        assert!(defs["Widget"]["properties"].get("debug").is_none());
    }

    #[test]
    fn external_ref_to_a_private_root_stays_unresolved() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("Hidden.yaml"),
            "type: object\nx-visibility:\n  extent: private\n",
        )
        .expect("write model");
        let spec_path = dir.path().join("openapi.yaml");
        std::fs::write(
            &spec_path,
            "openapi: '3.0.0'\ncomponents:\n  schemas:\n    Widget:\n      $ref: './Hidden.yaml'\n",
        )
        .expect("write spec");

        let (spec, defs) =
            crate::dereference::load_and_dereference(&spec_path, None).expect("dereference");

        assert!(defs.is_empty(), "private file must not be lifted: {defs:?}");
        assert_eq!(
            crate::dereference::collect_unresolved_refs(&spec),
            ["./Hidden.yaml"]
        );
    }

    #[test]
    fn retain_reachable_defs_keeps_transitive_and_mapped_targets() {
        let catalog = json!({
            "endpoints": [{ "schema": { "$ref": "#/$defs/Order" } }]
        });
        let mut defs: IndexMap<String, Value> = [
            ("Order", json!({ "items": { "$ref": "#/$defs/Line" } })),
            (
                "Line",
                json!({ "discriminator": { "mapping": { "gift": "#/$defs/Gift" } } }),
            ),
            ("Gift", json!({})),
            ("a/b", json!({})),
            ("Stranded", json!({ "$ref": "#/$defs/Gift" })),
        ]
        .into_iter()
        .map(|(name, def)| (name.to_owned(), def))
        .collect();
        // A key with a slash is referenced through its escaped pointer form.
        let catalog =
            json!({ "endpoints": [catalog["endpoints"][0].clone(), { "$ref": "#/$defs/a~1b" }] });

        assert_eq!(retain_reachable_defs(&catalog, &mut defs), 1);
        assert_eq!(
            defs.keys().map(String::as_str).collect::<Vec<_>>(),
            ["Order", "Line", "Gift", "a/b"]
        );
    }

    #[test]
    fn private_primary_spec_is_rejected() {
        let dir = tempfile::tempdir().expect("tempdir");
        let spec_path = dir.path().join("openapi.yaml");
        std::fs::write(
            &spec_path,
            "openapi: '3.0.0'\nx-visibility:\n  extent: private\npaths:\n  /a:\n    get: {}\n",
        )
        .expect("write spec");

        let err = crate::dereference::load_and_dereference(&spec_path, None)
            .expect_err("private root must fail");
        assert!(err.to_string().contains("marked private"), "{err}");
    }

    #[test]
    fn public_reference_to_a_pruned_local_component_fails() {
        let dir = tempfile::tempdir().expect("tempdir");
        let spec_path = dir.path().join("openapi.yaml");
        std::fs::write(
            &spec_path,
            "openapi: '3.0.0'\npaths:\n  /a:\n    get:\n      responses:\n        '200':\n          content:\n            application/json:\n              schema:\n                $ref: '#/components/schemas/Hidden'\ncomponents:\n  schemas:\n    Hidden:\n      type: object\n      x-visibility:\n        extent: private\n",
        )
        .expect("write spec");

        let err = crate::dereference::load_and_dereference(&spec_path, None)
            .expect_err("dangling local ref must fail");
        assert!(
            err.to_string().contains("#/components/schemas/Hidden"),
            "{err}"
        );
    }

    #[test]
    fn path_item_with_an_unresolved_ref_is_kept_for_validation() {
        let mut spec = json!({ "paths": { "/hidden": { "$ref": "./Hidden.yaml" } } });
        drop_operationless_paths(&mut spec);
        assert!(spec["paths"].get("/hidden").is_some());
    }

    #[test]
    fn reference_that_was_already_dangling_is_not_blamed_on_the_filter() {
        let dir = tempfile::tempdir().expect("tempdir");
        let spec_path = dir.path().join("openapi.yaml");
        std::fs::write(
            &spec_path,
            "openapi: '3.0.0'\npaths:\n  /a:\n    get:\n      parameters:\n        - $ref: '#/components/parameters/missing'\n",
        )
        .expect("write spec");

        crate::dereference::load_and_dereference(&spec_path, None)
            .expect("an upstream-dangling ref is not a private-node error");
    }
}
