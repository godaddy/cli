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
use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use serde_json::Value;

use crate::github::parse_yaml_or_json;
use crate::openapi::HTTP_METHODS;
use crate::ref_paths::normalize_path;

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

/// Fails if `spec` — already pruned — still points into a node the filter
/// removed (see [`private_pointers`]). Leaving such a reference in place makes
/// dereferencing quietly produce a broken or wrong schema (a name-keyed cache
/// can even satisfy it with an unrelated one), and `collect_unresolved_refs`
/// ignores local pointers, so this runs on the raw document before any `$ref`
/// is resolved. Counts `$ref`s and `discriminator.mapping` values, which carry
/// references as plain strings.
fn ensure_no_refs_to_private(spec_file: &Path, private: &[String], spec: &Value) -> Result<()> {
    let dir = spec_file.parent().unwrap_or(spec_file);
    let mut found = Vec::new();
    collect_refs_to_private(spec, private, dir, &mut found);
    found.sort();
    found.dedup();
    if !found.is_empty() {
        bail!(
            "{} reference(s) in {} point at nodes marked private:\n  {}",
            found.len(),
            spec_file.display(),
            found.join("\n  ")
        );
    }
    Ok(())
}

/// Whether `reference` is, or points inside, one of the `private` pointers.
/// `bare_name` is for `discriminator.mapping` values, where a bare schema name
/// means `#/components/schemas/<name>`; a `$ref` without a `#` names a file.
fn targets_private(reference: &str, private: &[String], bare_name: bool) -> bool {
    let expanded;
    let reference = if bare_name && !reference.contains(['/', '#']) {
        expanded = format!("#/components/schemas/{reference}");
        &expanded
    } else {
        reference
    };
    private.iter().any(|pointer| {
        reference == pointer
            || reference
                .strip_prefix(pointer.as_str())
                .is_some_and(|rest| rest.starts_with('/'))
    })
}

fn collect_refs_to_private(value: &Value, private: &[String], dir: &Path, out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            if let Some(Value::String(reference)) = map.get("$ref")
                && targets_private(reference, private, false)
            {
                out.push(reference.clone());
            }
            if let Some(mapping) = map
                .get("discriminator")
                .and_then(|discriminator| discriminator.get("mapping"))
                .and_then(Value::as_object)
            {
                out.extend(
                    mapping
                        .values()
                        .filter_map(Value::as_str)
                        .filter(|target| {
                            targets_private(target, private, true)
                                || (!target.starts_with('#') && targets_private_node(target, dir))
                        })
                        .map(str::to_owned),
                );
            }
            for child in map.values() {
                collect_refs_to_private(child, private, dir, out);
            }
        }
        Value::Array(values) => {
            for child in values {
                collect_refs_to_private(child, private, dir, out);
            }
        }
        _ => {}
    }
}

/// Filters a freshly parsed spec document before it is dereferenced. Fails if
/// the document is itself marked private — pruning only removes marked
/// children, so a private root would otherwise export all of its unmarked
/// content — or if anything left in it still points at a pruned node.
pub(crate) fn prune_spec(spec_file: &Path, parsed: &mut Value) -> Result<()> {
    if is_private(parsed) {
        bail!(
            "{} is marked private (x-visibility) and can't be synced",
            spec_file.display()
        );
    }
    let private = private_pointers(parsed);
    prune_private(parsed);
    ensure_no_refs_to_private(spec_file, &private, parsed)
}

/// [`prune_spec`] for a `$ref`'d file. A rejected file yields `None`, so the
/// `$ref` to it stays unresolved and fails the sync instead of exposing it.
pub(crate) fn prune_ref_file(file: &Path, parsed: &mut Value) -> Option<()> {
    prune_spec(file, parsed)
        .map_err(|e| eprintln!("WARNING: ignoring ref file: {e}"))
        .ok()
}

/// Reads the file a relative file `$ref` names, with its fragment, if it can be
/// located and mentions the visibility marker at all (a file that never does
/// can't be private or hold private nodes, so it isn't parsed). Covers the
/// same two locations as a normal load (beside the spec, or in its parent
/// directory); URL refs and case-insensitive matches are not probed.
fn read_marked_ref_file(ref_str: &str, spec_dir: &Path) -> Option<(PathBuf, Value, String)> {
    let (file_part, fragment) = ref_str.split_once('#').unwrap_or((ref_str, ""));
    if file_part.is_empty() || file_part.contains("://") {
        return None;
    }
    let path = [Some(spec_dir), spec_dir.parent()]
        .into_iter()
        .flatten()
        .map(|dir| normalize_path(&dir.join(file_part)))
        .find(|path| path.is_file())?;
    let src = std::fs::read_to_string(&path).ok()?;
    let parsed = src
        .contains("x-visibility")
        .then(|| parse_yaml_or_json(&src, &path).ok())??;
    Some((path, parsed, fragment.to_owned()))
}

/// Whether `ref_str`, a relative file `$ref`, targets private content: a file
/// whose own root is private, or a fragment at or inside a private node of it.
/// Shallow on purpose — it never follows the target's own references — so
/// discriminator mappings can use it without risking a cycle.
fn targets_private_node(ref_str: &str, spec_dir: &Path) -> bool {
    read_marked_ref_file(ref_str, spec_dir).is_some_and(|(_, parsed, fragment)| {
        is_private(&parsed)
            || (!fragment.is_empty()
                && targets_private(&format!("#{fragment}"), &private_pointers(&parsed), false))
    })
}

/// Whether loading the file `ref_str` names would have been rejected: it is
/// private, the fragment is inside a private node, or [`prune_spec`] would fail
/// on it (it still points at one of its own private nodes). The
/// lifted-definitions cache is keyed by name, so such a reference can hit the
/// entry of a same-named public schema without the target ever being checked;
/// callers check this first and leave the reference unresolved.
pub(crate) fn ref_targets_private_file(ref_str: &str, spec_dir: &Path) -> bool {
    if targets_private_node(ref_str, spec_dir) {
        return true;
    }
    read_marked_ref_file(ref_str, spec_dir)
        .is_some_and(|(path, mut parsed, _)| prune_spec(&path, &mut parsed).is_err())
}

/// Drops what pruning left empty in a dereferenced spec: path items with no
/// operations (non-path `paths` keys, `x-*` extensions, are left alone) and
/// top-level tags no remaining operation uses. Run after [`prune_private`], so a
/// path or tag whose every operation was private doesn't linger as an empty entry.
pub(crate) fn drop_emptied_paths_and_tags(spec: &mut Value) {
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
    let used: HashSet<String> = paths
        .values()
        .filter_map(Value::as_object)
        .flat_map(|item| HTTP_METHODS.iter().filter_map(|method| item.get(*method)))
        .filter_map(|operation| operation.get("tags").and_then(Value::as_array))
        .flatten()
        .filter_map(|tag| tag.as_str().map(str::to_owned))
        .collect();
    if let Some(tags) = spec.get_mut("tags").and_then(Value::as_array_mut) {
        tags.retain(|tag| {
            tag.get("name")
                .and_then(Value::as_str)
                .is_none_or(|name| used.contains(name))
        });
    }
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
        drop_emptied_paths_and_tags(&mut spec);
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
        drop_emptied_paths_and_tags(&mut spec);
        assert_eq!(spec, original);
    }

    #[test]
    fn keeps_non_path_extensions_in_paths() {
        let mut spec = json!({ "paths": { "x-note": {}, "/a": {} } });
        drop_emptied_paths_and_tags(&mut spec);
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
        drop_emptied_paths_and_tags(&mut spec);
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

    #[test]
    fn external_file_referencing_its_own_private_node_is_rejected() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("Widget.yaml"),
            "type: object\nproperties:\n  hidden:\n    $ref: '#/definitions/Hidden'\ndefinitions:\n  Hidden:\n    type: object\n    x-visibility:\n      extent: private\n",
        )
        .expect("write model");
        let spec_path = dir.path().join("openapi.yaml");
        std::fs::write(
            &spec_path,
            "openapi: '3.0.0'\ncomponents:\n  schemas:\n    Widget:\n      $ref: './Widget.yaml'\n",
        )
        .expect("write spec");

        let (spec, defs) =
            crate::dereference::load_and_dereference(&spec_path, None).expect("dereference");

        assert!(
            defs.is_empty(),
            "rejected file must not be lifted: {defs:?}"
        );
        assert_eq!(
            crate::dereference::collect_unresolved_refs(&spec),
            ["./Widget.yaml"]
        );
    }

    #[test]
    fn discriminator_mapping_to_a_pruned_schema_fails() {
        for target in ["'#/components/schemas/Hidden'", "Hidden"] {
            let dir = tempfile::tempdir().expect("tempdir");
            let spec_path = dir.path().join("openapi.yaml");
            std::fs::write(
                &spec_path,
                format!(
                    "openapi: '3.0.0'\ncomponents:\n  schemas:\n    Pet:\n      type: object\n      discriminator:\n        propertyName: kind\n        mapping:\n          hidden: {target}\n    Hidden:\n      type: object\n      x-visibility:\n        extent: private\n"
                ),
            )
            .expect("write spec");

            let err = crate::dereference::load_and_dereference(&spec_path, None)
                .expect_err("mapping to a private schema must fail");
            assert!(
                err.to_string().contains("marked private"),
                "{target}: {err}"
            );
        }
    }

    #[test]
    fn discriminator_mapping_to_a_public_schema_is_kept() {
        let dir = tempfile::tempdir().expect("tempdir");
        let spec_path = dir.path().join("openapi.yaml");
        std::fs::write(
            &spec_path,
            "openapi: '3.0.0'\ncomponents:\n  schemas:\n    Pet:\n      type: object\n      discriminator:\n        propertyName: kind\n        mapping:\n          dog: Dog\n    Dog:\n      type: object\n    Hidden:\n      type: object\n      x-visibility:\n        extent: private\n",
        )
        .expect("write spec");

        crate::dereference::load_and_dereference(&spec_path, None)
            .expect("a public mapping target is not an error");
    }

    #[test]
    fn name_keyed_def_cache_cannot_satisfy_a_reference_to_a_pruned_schema() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("Hidden.yaml"), "type: string\n").expect("write model");
        let spec_path = dir.path().join("openapi.yaml");
        std::fs::write(
            &spec_path,
            "openapi: '3.0.0'\ncomponents:\n  schemas:\n    Other:\n      $ref: './Hidden.yaml'\n    Uses:\n      $ref: '#/components/schemas/Hidden'\n    Hidden:\n      type: object\n      x-visibility:\n        extent: private\n",
        )
        .expect("write spec");

        let err = crate::dereference::load_and_dereference(&spec_path, None)
            .expect_err("reference to the pruned local schema must fail");
        assert!(
            err.to_string().contains("#/components/schemas/Hidden"),
            "{err}"
        );
    }

    #[test]
    fn external_file_cannot_reference_its_private_node_through_its_own_placeholder() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("Widget.yaml"),
            "type: object\nproperties:\n  next:\n    $ref: '#/definitions/Widget'\ndefinitions:\n  Widget:\n    type: object\n    x-visibility:\n      extent: private\n",
        )
        .expect("write model");
        let spec_path = dir.path().join("openapi.yaml");
        std::fs::write(
            &spec_path,
            "openapi: '3.0.0'\ncomponents:\n  schemas:\n    Widget:\n      $ref: './Widget.yaml'\n",
        )
        .expect("write spec");

        let (spec, defs) =
            crate::dereference::load_and_dereference(&spec_path, None).expect("dereference");

        assert!(
            defs.is_empty(),
            "rejected file must not be lifted: {defs:?}"
        );
        assert_eq!(
            crate::dereference::collect_unresolved_refs(&spec),
            ["./Widget.yaml"]
        );
    }

    #[test]
    fn private_file_is_not_replaced_by_a_public_one_in_the_fallback_directory() {
        let dir = tempfile::tempdir().expect("tempdir");
        let schemas = dir.path().join("schemas");
        std::fs::create_dir_all(schemas.join("models")).expect("create dirs");
        std::fs::create_dir_all(dir.path().join("models")).expect("create dirs");
        std::fs::write(
            schemas.join("models/Hidden.yaml"),
            "type: object\nx-visibility:\n  extent: private\n",
        )
        .expect("write private model");
        std::fs::write(dir.path().join("models/Hidden.yaml"), "type: string\n")
            .expect("write public model");
        let spec_path = schemas.join("openapi.yaml");
        std::fs::write(
            &spec_path,
            "openapi: '3.0.0'\ncomponents:\n  schemas:\n    Widget:\n      $ref: 'models/Hidden.yaml'\n",
        )
        .expect("write spec");

        let (spec, defs) =
            crate::dereference::load_and_dereference(&spec_path, None).expect("dereference");

        assert!(
            defs.is_empty(),
            "fallback file must not be lifted: {defs:?}"
        );
        assert_eq!(
            crate::dereference::collect_unresolved_refs(&spec),
            ["models/Hidden.yaml"]
        );
    }

    #[cfg(unix)]
    #[test]
    fn case_insensitive_match_to_a_private_file_does_not_fall_back() {
        let dir = tempfile::tempdir().expect("tempdir");
        let schemas = dir.path().join("schemas");
        std::fs::create_dir_all(schemas.join("models")).expect("create dirs");
        std::fs::create_dir_all(dir.path().join("models")).expect("create dirs");
        // The ref says `Hidden.yaml`; on disk the private file is `hidden.yaml`,
        // so it is only found by the case-insensitive lookup.
        std::fs::write(
            schemas.join("models/hidden.yaml"),
            "type: object\nx-visibility:\n  extent: private\n",
        )
        .expect("write private model");
        std::fs::write(dir.path().join("models/Hidden.yaml"), "type: string\n")
            .expect("write public model");
        let spec_path = schemas.join("openapi.yaml");
        std::fs::write(
            &spec_path,
            "openapi: '3.0.0'\ncomponents:\n  schemas:\n    Widget:\n      $ref: 'models/Hidden.yaml'\n",
        )
        .expect("write spec");

        let (spec, defs) =
            crate::dereference::load_and_dereference(&spec_path, None).expect("dereference");

        assert!(
            defs.is_empty(),
            "fallback file must not be lifted: {defs:?}"
        );
        assert_eq!(
            crate::dereference::collect_unresolved_refs(&spec),
            ["models/Hidden.yaml"]
        );
    }

    #[test]
    fn ref_to_a_file_is_not_mistaken_for_a_private_schema_name() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("Widget.yaml"), "type: string\n").expect("write model");
        let spec_path = dir.path().join("openapi.yaml");
        std::fs::write(
            &spec_path,
            "openapi: '3.0.0'\ncomponents:\n  schemas:\n    Uses:\n      $ref: 'Widget.yaml'\n    Widget.yaml:\n      type: object\n      x-visibility:\n        extent: private\n",
        )
        .expect("write spec");

        crate::dereference::load_and_dereference(&spec_path, None)
            .expect("a file ref is not a bare schema name");
    }

    #[test]
    fn reference_to_a_private_file_is_not_satisfied_by_a_same_named_public_one() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(dir.path().join("public")).expect("create dir");
        std::fs::create_dir_all(dir.path().join("private")).expect("create dir");
        std::fs::write(dir.path().join("public/Widget.yaml"), "type: string\n")
            .expect("write public model");
        std::fs::write(
            dir.path().join("private/Widget.yaml"),
            "type: object\nx-visibility:\n  extent: private\n",
        )
        .expect("write private model");
        let spec_path = dir.path().join("openapi.yaml");
        // Components are visited in key order, so the public file is lifted first.
        std::fs::write(
            &spec_path,
            "openapi: '3.0.0'\ncomponents:\n  schemas:\n    A:\n      $ref: './public/Widget.yaml'\n    B:\n      $ref: './private/Widget.yaml'\n",
        )
        .expect("write spec");

        let (spec, defs) =
            crate::dereference::load_and_dereference(&spec_path, None).expect("dereference");

        assert_eq!(defs["Widget"], json!({ "type": "string" }));
        assert_eq!(
            crate::dereference::collect_unresolved_refs(&spec),
            ["./private/Widget.yaml"]
        );
    }

    #[test]
    fn tags_with_no_remaining_operation_are_dropped() {
        let mut spec = json!({
            "tags": [{ "name": "Renewals" }, { "name": "Domains" }],
            "paths": {
                "/domains": { "get": { "tags": ["Domains"] } },
                "/renewals": { "post": { "tags": ["Renewals"], "x-visibility": { "extent": "private" } } }
            }
        });
        prune_private(&mut spec);
        drop_emptied_paths_and_tags(&mut spec);
        assert_eq!(spec["tags"], json!([{ "name": "Domains" }]));
    }

    #[test]
    fn fragment_into_a_private_node_of_a_public_file_is_not_satisfied_by_the_cache() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(dir.path().join("public")).expect("create dir");
        std::fs::write(dir.path().join("public/Hidden.yaml"), "type: string\n")
            .expect("write public model");
        std::fs::write(
            dir.path().join("models.yaml"),
            "components:\n  schemas:\n    Hidden:\n      type: object\n      x-visibility:\n        extent: private\n    Shown:\n      type: object\n",
        )
        .expect("write models");
        let spec_path = dir.path().join("openapi.yaml");
        // Components are visited in key order, so the public file is lifted first.
        std::fs::write(
            &spec_path,
            "openapi: '3.0.0'\ncomponents:\n  schemas:\n    A:\n      $ref: './public/Hidden.yaml'\n    B:\n      $ref: './models.yaml#/components/schemas/Hidden'\n",
        )
        .expect("write spec");

        let (spec, defs) =
            crate::dereference::load_and_dereference(&spec_path, None).expect("dereference");

        assert_eq!(defs["Hidden"], json!({ "type": "string" }));
        assert_eq!(
            crate::dereference::collect_unresolved_refs(&spec),
            ["./models.yaml#/components/schemas/Hidden"]
        );
    }

    #[test]
    fn external_discriminator_mapping_to_a_private_file_fails() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(dir.path().join("public")).expect("create dir");
        std::fs::create_dir_all(dir.path().join("private")).expect("create dir");
        std::fs::write(dir.path().join("public/Widget.yaml"), "type: object\n")
            .expect("write public model");
        std::fs::write(
            dir.path().join("private/Widget.yaml"),
            "type: object\nx-visibility:\n  extent: private\n",
        )
        .expect("write private model");
        let spec_path = dir.path().join("openapi.yaml");
        std::fs::write(
            &spec_path,
            "openapi: '3.0.0'\ncomponents:\n  schemas:\n    Pet:\n      oneOf:\n        - $ref: './public/Widget.yaml'\n      discriminator:\n        propertyName: kind\n        mapping:\n          widget: './private/Widget.yaml'\n",
        )
        .expect("write spec");

        let err = crate::dereference::load_and_dereference(&spec_path, None)
            .expect_err("a mapping to a private file must fail");
        assert!(err.to_string().contains("./private/Widget.yaml"), "{err}");
    }

    #[test]
    fn external_discriminator_mapping_to_a_public_file_is_kept() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(dir.path().join("public")).expect("create dir");
        std::fs::write(dir.path().join("public/Widget.yaml"), "type: object\n")
            .expect("write public model");
        let spec_path = dir.path().join("openapi.yaml");
        std::fs::write(
            &spec_path,
            "openapi: '3.0.0'\ncomponents:\n  schemas:\n    Pet:\n      oneOf:\n        - $ref: './public/Widget.yaml'\n      discriminator:\n        propertyName: kind\n        mapping:\n          widget: './public/Widget.yaml'\n",
        )
        .expect("write spec");

        crate::dereference::load_and_dereference(&spec_path, None)
            .expect("a mapping to a public file is not an error");
    }

    #[test]
    fn cache_hit_does_not_hide_a_same_named_file_that_references_its_private_node() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(dir.path().join("public")).expect("create dir");
        std::fs::create_dir_all(dir.path().join("other")).expect("create dir");
        std::fs::write(dir.path().join("public/Widget.yaml"), "type: string\n")
            .expect("write public model");
        std::fs::write(
            dir.path().join("other/Widget.yaml"),
            "properties:\n  hidden:\n    $ref: '#/definitions/Hidden'\ndefinitions:\n  Hidden:\n    x-visibility:\n      extent: private\n",
        )
        .expect("write model with a dangling private ref");
        let spec_path = dir.path().join("openapi.yaml");
        // Components are visited in key order, so the public file is lifted first.
        std::fs::write(
            &spec_path,
            "openapi: '3.0.0'\ncomponents:\n  schemas:\n    A:\n      $ref: './public/Widget.yaml'\n    B:\n      $ref: './other/Widget.yaml'\n",
        )
        .expect("write spec");

        let (spec, defs) =
            crate::dereference::load_and_dereference(&spec_path, None).expect("dereference");

        assert_eq!(defs["Widget"], json!({ "type": "string" }));
        assert_eq!(
            crate::dereference::collect_unresolved_refs(&spec),
            ["./other/Widget.yaml"]
        );
    }

    #[test]
    fn discriminator_mappings_that_point_at_each_other_do_not_loop() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("A.yaml"),
            "discriminator:\n  propertyName: k\n  mapping:\n    b: './B.yaml'\nx-note: x-visibility\n",
        )
        .expect("write A");
        std::fs::write(
            dir.path().join("B.yaml"),
            "discriminator:\n  propertyName: k\n  mapping:\n    a: './A.yaml'\nx-note: x-visibility\n",
        )
        .expect("write B");
        let spec_path = dir.path().join("openapi.yaml");
        std::fs::write(
            &spec_path,
            "openapi: '3.0.0'\ncomponents:\n  schemas:\n    X:\n      $ref: './A.yaml'\n    Y:\n      $ref: './A.yaml'\n",
        )
        .expect("write spec");

        crate::dereference::load_and_dereference(&spec_path, None)
            .expect("mutually referencing mappings terminate");
    }
}
