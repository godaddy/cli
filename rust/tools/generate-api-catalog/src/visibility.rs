//! Pre-filter for spec nodes marked private.
//!
//! Upstream specs can carry `x-visibility: { extent: private }` at any level
//! (an operation, a schema, a property, a tag, ...) to say the node is not part
//! of the public API surface. Everything the CLI derives from a spec — the
//! catalog and the progenitor-codegen documents — must leave those nodes out,
//! so [`prune_private`] strips them from each document as it is loaded, before
//! any `$ref` is resolved. A private node is therefore never lifted into
//! `$defs`, and a `$ref` that points at one is left unresolved.

use serde_json::Value;

use crate::openapi::HTTP_METHODS;

fn is_private(value: &Value) -> bool {
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
            || item
                .as_object()
                .is_none_or(|item| HTTP_METHODS.iter().any(|method| item.contains_key(*method)))
    });
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
}
