use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use indexmap::IndexMap;
use serde_json::Value;

use crate::github::parse_yaml_or_json;

// ---------------------------------------------------------------------------
// $ref resolution
// ---------------------------------------------------------------------------

fn resolve_local_ref<'a>(root: &'a Value, pointer: &str) -> Option<&'a Value> {
    let path = pointer.strip_prefix("#/")?;
    let mut cur = root;
    for seg in path.split('/') {
        let seg = seg.replace("~1", "/").replace("~0", "~");
        cur = cur.get(&seg)?;
    }
    Some(cur)
}

/// Returns the `$defs` key to use when lifting a `$ref` string, or `None` if the ref
/// should be inlined as before.
///
/// Lifted: external file/URL refs, and `#/components/schemas/` / `#/definitions/` local refs.
/// Inlined: all other local refs (e.g. `#/paths/...`).
fn should_lift_to_defs(ref_str: &str) -> Option<String> {
    // External file or URL: lift using a key derived from the file name
    if !ref_str.starts_with('#') {
        return Some(derive_defs_key_for_path(ref_str));
    }
    // Local schema component refs
    for prefix in &["#/components/schemas/", "#/definitions/"] {
        if let Some(rest) = ref_str.strip_prefix(prefix) {
            return Some(rest.to_owned());
        }
    }
    None
}

/// Escape a string for use as a single JSON Pointer segment (RFC 6901).
/// `~` → `~0`, `/` → `~1`.
fn json_pointer_escape(s: &str) -> String {
    s.replace('~', "~0").replace('/', "~1")
}

/// Derive a stable `$defs` key from an external ref path or URL.
/// Examples:
///   `./models/Order.yaml`  → `"Order"`
///   `https://.../error.json` → `"error"`
fn derive_defs_key_for_path(ref_str: &str) -> String {
    let frag = ref_str.find('#').map(|idx| &ref_str[idx..]);

    // External refs with a component-schema fragment (e.g. `./common.yaml#/components/schemas/Foo`)
    // use the component name as the key, consistent with pure local `#/components/schemas/` refs
    // and avoiding collisions when the same file is referenced for different components.
    if let Some(frag) = frag {
        for prefix in &["#/components/schemas/", "#/definitions/"] {
            if let Some(rest) = frag.strip_prefix(prefix) {
                let name = rest.split('/').next().unwrap_or(rest);
                return sanitize_defs_key(name);
            }
        }
    }

    let file_part = ref_str.split('#').next().unwrap_or(ref_str);
    let last_seg = file_part.rsplit(['/', '\\']).next().unwrap_or(file_part);
    let stem = match last_seg.rfind('.') {
        Some(pos) => &last_seg[..pos],
        None => last_seg,
    };
    let base_key = sanitize_defs_key(stem);

    // A `#/properties/<name>` fragment (optionally nested, e.g. `#/properties/a/properties/b`)
    // selects one property's schema out of the file, not the file's root schema — fold the
    // property path into the key so two refs into the same file for different properties
    // don't collide under one bare file-stem key. Each segment has its literal `_` doubled
    // before joining on a single `_`, so a property literally named `a_b` can't collide with
    // nested segments `a` + `b` (both would otherwise sanitize to the same `a_b` suffix).
    match frag.and_then(|f| f.strip_prefix("#/properties/")) {
        Some(rest) => {
            let suffix = rest
                .split("/properties/")
                .map(|seg| seg.replace('_', "__"))
                .collect::<Vec<_>>()
                .join("_");
            sanitize_defs_key(&format!("{base_key}_{suffix}"))
        }
        None => base_key,
    }
}

fn sanitize_defs_key(s: &str) -> String {
    let sanitized: String = s
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    sanitized.trim_matches('_').to_owned()
}

fn dereference(
    root: &Value,
    current: Value,
    spec_dir: &Path,
    common_types_dir: Option<&Path>,
    defs: &mut IndexMap<String, Value>,
    depth: usize,
) -> Value {
    if depth > 64 {
        return current;
    }
    match current {
        Value::Object(mut map) => {
            if let Some(Value::String(ref_str)) = map.get("$ref").cloned() {
                if let Some(key) = should_lift_to_defs(&ref_str) {
                    // Already registered — return the local pointer immediately (dedup)
                    if defs.contains_key(&key) {
                        return serde_json::json!({"$ref": format!("#/$defs/{}", json_pointer_escape(&key))});
                    }
                    // Insert null placeholder before resolving (circular ref guard)
                    defs.insert(key.clone(), Value::Null);
                    let resolved_opt = if ref_str.starts_with('#') {
                        // Local component ref: resolve from this spec's root
                        resolve_local_ref(root, &ref_str).cloned().map(|v| {
                            dereference(root, v, spec_dir, common_types_dir, defs, depth + 1)
                        })
                    } else {
                        // External file/URL ref
                        resolve_ref(root, &ref_str, spec_dir, common_types_dir, defs, depth + 1)
                    };
                    if let Some(content) = resolved_opt {
                        defs.insert(key.clone(), content);
                        return serde_json::json!({"$ref": format!("#/$defs/{}", json_pointer_escape(&key))});
                    }
                    // Resolution failed — remove placeholder and preserve original $ref
                    // so collect_unresolved_refs() will catch it.
                    defs.shift_remove(&key);
                    return Value::Object(map);
                }
                // Non-liftable local ref (e.g. #/paths/...) — inline as before
                let resolved =
                    resolve_ref(root, &ref_str, spec_dir, common_types_dir, defs, depth + 1);
                return resolved
                    .map(|v| dereference(root, v, spec_dir, common_types_dir, defs, depth + 1))
                    .unwrap_or(Value::Object(map));
            }
            // Recurse into all values
            for v in map.values_mut() {
                *v = dereference(root, v.take(), spec_dir, common_types_dir, defs, depth + 1);
            }
            Value::Object(map)
        }
        Value::Array(mut arr) => {
            for v in &mut arr {
                *v = dereference(root, v.take(), spec_dir, common_types_dir, defs, depth + 1);
            }
            Value::Array(arr)
        }
        other => other,
    }
}

fn resolve_ref(
    root: &Value,
    ref_str: &str,
    spec_dir: &Path,
    common_types_dir: Option<&Path>,
    defs: &mut IndexMap<String, Value>,
    depth: usize,
) -> Option<Value> {
    if ref_str.starts_with("#/") {
        return resolve_local_ref(root, ref_str).cloned();
    }

    if ref_str.starts_with("https://schemas.api.godaddy.com/") {
        let ct_dir = common_types_dir?;
        let url_path = ref_str
            .strip_prefix("https://schemas.api.godaddy.com")
            .unwrap_or("")
            .strip_prefix("/common-types")
            .unwrap_or(
                ref_str
                    .strip_prefix("https://schemas.api.godaddy.com")
                    .unwrap_or(""),
            );
        let local_path = ct_dir.join(url_path.trim_start_matches('/'));
        return load_external_ref(&local_path, common_types_dir, defs, depth);
    }

    // Relative file reference — strip fragment
    let (file_part, fragment) = match ref_str.find('#') {
        Some(i) => (&ref_str[..i], Some(&ref_str[i..])),
        None => (ref_str, None),
    };

    if file_part.is_empty() {
        // Same-file fragment reference
        return resolve_local_ref(root, fragment?).cloned();
    }

    let file_path = spec_dir.join(file_part);
    // Some repos store model files alongside the `schemas/` directory rather than
    // inside it (e.g. `v2/models/` instead of `v2/schemas/models/`).  Try the
    // parent of spec_dir as a fallback so both layouts resolve correctly.
    let external_root =
        load_external_ref(&file_path, common_types_dir, defs, depth).or_else(|| {
            spec_dir.parent().and_then(|parent| {
                load_external_ref(&parent.join(file_part), common_types_dir, defs, depth)
            })
        })?;

    if let Some(frag) = fragment
        && !frag.is_empty()
    {
        return resolve_local_ref(&external_root, frag).cloned();
    }
    Some(external_root)
}

fn load_external_ref(
    path: &Path,
    common_types_dir: Option<&Path>,
    defs: &mut IndexMap<String, Value>,
    depth: usize,
) -> Option<Value> {
    // Normalize path (resolve .. segments) so the common-types check below works on
    // paths like ./models/../common-types/v1/schemas/yaml/uuid.yaml
    let normalized = normalize_path(path);
    let resolved = if normalized.exists() {
        normalized.clone()
    } else if let Some(fallback) = resolve_common_types_path(&normalized, common_types_dir) {
        fallback
    } else if let Some(ci) = resolve_case_insensitive(&normalized) {
        // Spec files authored on case-insensitive filesystems (macOS) sometimes
        // have mismatched filename casing.  Accept the match but warn so the
        // spec repo can be fixed.
        eprintln!(
            "WARNING: case-insensitive match for {}: using {}",
            path.display(),
            ci.display()
        );
        ci
    } else {
        eprintln!(
            "WARNING: cannot read ref file {}: No such file or directory (os error 2)",
            path.display()
        );
        return None;
    };

    let src = std::fs::read_to_string(&resolved)
        .map_err(|e| eprintln!("WARNING: cannot read ref file {}: {e}", resolved.display()))
        .ok()?;
    let parsed = parse_yaml_or_json(&src, &resolved)
        .map_err(|e| eprintln!("WARNING: cannot parse ref file {}: {e}", resolved.display()))
        .ok()?;
    let dir = resolved.parent().unwrap_or(&resolved);
    Some(dereference(
        &parsed.clone(),
        parsed,
        dir,
        common_types_dir,
        defs,
        depth + 1,
    ))
}

/// Try to find `path` in its parent directory with a case-insensitive filename match.
/// Returns the first candidate whose lowercased name equals the lowercased target,
/// or `None` if no unique match is found.
fn resolve_case_insensitive(path: &Path) -> Option<PathBuf> {
    let dir = path.parent()?;
    let target = path.file_name()?.to_string_lossy().to_lowercase();
    let mut matches: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().to_lowercase() == target)
        .map(|e| e.path())
        .collect();
    if matches.len() == 1 {
        matches.pop()
    } else {
        None
    }
}

/// Normalize a path by resolving `.` and `..` components without hitting the filesystem.
fn normalize_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            c => out.push(c),
        }
    }
    out
}

/// When a path contains a `common-types` segment and does not exist, try to locate the
/// file in the standalone common-types-specification clone. Mirrors the TypeScript
/// `resolveCommonTypesFile` helper.
fn resolve_common_types_path(path: &Path, common_types_dir: Option<&Path>) -> Option<PathBuf> {
    let ct_dir = common_types_dir?;
    let path_str = path.to_string_lossy();
    let ct_marker = "common-types/";
    let idx = path_str.find(ct_marker)?;

    let rel = &path_str[idx + ct_marker.len()..];
    // 1. Try path as-is relative to the clone root
    let direct = ct_dir.join(rel);
    if direct.exists() {
        return Some(direct);
    }

    // 2. Try v1/schemas/{yaml,json}/<basename> (covering both extension variants)
    let basename = path.file_name()?;
    let basename_str = basename.to_string_lossy();
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let (primary_sub, alt_sub, alt_ext) = if ext == "json" {
        ("json", "yaml", "yaml")
    } else {
        ("yaml", "json", "json")
    };

    let nested = ct_dir
        .join("v1")
        .join("schemas")
        .join(primary_sub)
        .join(basename_str.as_ref());
    if nested.exists() {
        return Some(nested);
    }
    let stem = path.file_stem()?.to_string_lossy();
    let alt_name = format!("{stem}.{alt_ext}");
    let alt = ct_dir
        .join("v1")
        .join("schemas")
        .join(alt_sub)
        .join(&alt_name);
    if alt.exists() {
        return Some(alt);
    }

    None
}

/// Walk a dereferenced value and collect any remaining external `$ref` strings.
/// External refs are anything that is not a local JSON pointer (`#/...`).
pub(crate) fn collect_unresolved_refs(value: &Value) -> Vec<String> {
    let mut found = Vec::new();
    collect_unresolved_refs_inner(value, &mut found);
    found.sort();
    found.dedup();
    found
}

fn collect_unresolved_refs_inner(value: &Value, found: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            if let Some(Value::String(r)) = map.get("$ref")
                && !r.starts_with('#')
            {
                found.push(r.clone());
            }
            // `discriminator.mapping` values are schema refs carried as plain strings.
            // Any that still point outside this document (not `#/...`) are unresolved
            // and must fail the build, same as an external `$ref`.
            if let Some(Value::Object(disc)) = map.get("discriminator")
                && let Some(Value::Object(mapping)) = disc.get("mapping")
            {
                for v in mapping.values() {
                    if let Value::String(s) = v
                        && !s.starts_with('#')
                    {
                        found.push(s.clone());
                    }
                }
            }
            for v in map.values() {
                collect_unresolved_refs_inner(v, found);
            }
        }
        Value::Array(arr) => {
            for v in arr {
                collect_unresolved_refs_inner(v, found);
            }
        }
        _ => {}
    }
}

/// Rewrite `discriminator.mapping` values that still point at external files
/// (e.g. `./BasicChannel.yaml`, `../payment-token/ApplePayPaymentToken.yaml`) to
/// the local `#/$defs/<key>` pointer produced by ref-lifting.
///
/// The OpenAPI `discriminator.mapping` holds schema references as *plain strings*,
/// not `$ref` objects, so `dereference()` never touches them — leaving broken
/// external file paths in the generated catalog that surface through `api operation get`.
/// The referenced subschemas are lifted into `$defs` via the same key derivation
/// (`derive_defs_key_for_path`), so we rewrite each mapping value to the matching
/// `#/$defs/<key>` — but only when that def exists, so a mapping is never pointed
/// at a missing schema.
fn rewrite_discriminator_mappings(value: &mut Value, def_keys: &HashSet<String>) {
    match value {
        Value::Object(map) => {
            if let Some(Value::Object(disc)) = map.get_mut("discriminator")
                && let Some(Value::Object(mapping)) = disc.get_mut("mapping")
            {
                for v in mapping.values_mut() {
                    if let Value::String(s) = v
                        && !s.starts_with('#')
                    {
                        let key = derive_defs_key_for_path(s);
                        if def_keys.contains(&key) {
                            *s = format!("#/$defs/{}", json_pointer_escape(&key));
                        }
                    }
                }
            }
            for v in map.values_mut() {
                rewrite_discriminator_mappings(v, def_keys);
            }
        }
        Value::Array(arr) => {
            for v in arr {
                rewrite_discriminator_mappings(v, def_keys);
            }
        }
        _ => {}
    }
}

pub(crate) fn load_and_dereference(
    spec_file: &Path,
    common_types_dir: Option<&Path>,
) -> Result<(Value, IndexMap<String, Value>)> {
    let src = std::fs::read_to_string(spec_file)
        .with_context(|| format!("failed to read {}", spec_file.display()))?;
    let parsed = parse_yaml_or_json(&src, spec_file)?;
    let spec_dir = spec_file.parent().unwrap_or(spec_file);
    let mut defs = IndexMap::new();
    let mut dereffed = dereference(
        &parsed.clone(),
        parsed,
        spec_dir,
        common_types_dir,
        &mut defs,
        0,
    );
    // Rewrite discriminator mappings (which carry schema refs as plain strings) to
    // the lifted `#/$defs/<key>` pointers. Run after all lifting so every def key
    // is known; rewrite both the top-level spec and the lifted defs themselves.
    let def_keys: HashSet<String> = defs.keys().cloned().collect();
    rewrite_discriminator_mappings(&mut dereffed, &def_keys);
    for v in defs.values_mut() {
        rewrite_discriminator_mappings(v, &def_keys);
    }
    Ok((dereffed, defs))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn dereference_resolves_relative_file_ref() {
        let dir = tempfile::tempdir().expect("tempdir");
        let model_dir = dir.path().join("schemas").join("models");
        fs::create_dir_all(&model_dir).expect("create model dir");

        fs::write(
            model_dir.join("Widget.yaml"),
            "type: object\nproperties:\n  id:\n    type: string\n",
        )
        .expect("write model");

        let spec_path = dir.path().join("schemas").join("openapi.yaml");
        fs::write(
            &spec_path,
            "openapi: '3.0.0'\ncomponents:\n  schemas:\n    Widget:\n      $ref: './models/Widget.yaml'\n",
        )
        .expect("write spec");

        let (result, _defs) = load_and_dereference(&spec_path, None).expect("dereference");
        let unresolved = collect_unresolved_refs(&result);
        assert!(
            unresolved.is_empty(),
            "expected zero unresolved $refs, got: {unresolved:?}"
        );
    }

    #[test]
    fn dereference_resolves_ref_in_sibling_of_schemas_dir() {
        // Models at {dir}/models/ rather than {dir}/schemas/models/ (sibling layout)
        let dir = tempfile::tempdir().expect("tempdir");
        let model_dir = dir.path().join("models");
        fs::create_dir_all(&model_dir).expect("create model dir");
        let schemas_dir = dir.path().join("schemas");
        fs::create_dir_all(&schemas_dir).expect("create schemas dir");

        fs::write(
            model_dir.join("Widget.yaml"),
            "type: object\nproperties:\n  id:\n    type: string\n",
        )
        .expect("write model");

        let spec_path = schemas_dir.join("openapi.yaml");
        fs::write(
            &spec_path,
            "openapi: '3.0.0'\ncomponents:\n  schemas:\n    Widget:\n      $ref: './models/Widget.yaml'\n",
        )
        .expect("write spec");

        let (result, _defs) = load_and_dereference(&spec_path, None).expect("dereference");
        let unresolved = collect_unresolved_refs(&result);
        assert!(
            unresolved.is_empty(),
            "expected zero unresolved $refs with sibling layout, got: {unresolved:?}"
        );
    }

    #[test]
    fn collect_unresolved_refs_finds_external_refs() {
        let value = serde_json::json!({
            "components": {
                "schemas": {
                    "Foo": { "$ref": "./models/Foo.yaml" },
                    "Bar": { "$ref": "#/components/schemas/Baz" },
                    "Qux": { "type": "string" }
                }
            }
        });
        let unresolved = collect_unresolved_refs(&value);
        assert_eq!(unresolved, vec!["./models/Foo.yaml"]);
    }

    #[test]
    fn collect_unresolved_refs_finds_external_discriminator_mapping() {
        // A discriminator.mapping value pointing outside the document is unresolved,
        // even though it is a plain string rather than a `$ref` object.
        let value = serde_json::json!({
            "$defs": {
                "Channel": {
                    "discriminator": {
                        "propertyName": "kind",
                        "mapping": {
                            "DEFAULT": "./BasicChannel.yaml",
                            "LOCAL": "#/$defs/LocalChannel"
                        }
                    }
                }
            }
        });
        let unresolved = collect_unresolved_refs(&value);
        assert_eq!(unresolved, vec!["./BasicChannel.yaml"]);
    }

    #[test]
    fn discriminator_mapping_rewritten_to_defs_pointer() {
        let dir = tempfile::tempdir().expect("tempdir");
        let schemas_dir = dir.path().join("schemas");
        fs::create_dir_all(&schemas_dir).expect("create schemas dir");

        fs::write(
            schemas_dir.join("BasicChannel.yaml"),
            "type: object\nproperties:\n  kind:\n    type: string\n",
        )
        .expect("write model");

        // The oneOf `$ref` lifts BasicChannel into `$defs`; the discriminator.mapping
        // carries the same target as a plain string that dereference() won't touch.
        let spec_path = schemas_dir.join("openapi.yaml");
        fs::write(
            &spec_path,
            concat!(
                "openapi: '3.0.0'\n",
                "components:\n",
                "  schemas:\n",
                "    Channel:\n",
                "      oneOf:\n",
                "        - $ref: './BasicChannel.yaml'\n",
                "      discriminator:\n",
                "        propertyName: kind\n",
                "        mapping:\n",
                "          DEFAULT: './BasicChannel.yaml'\n",
            ),
        )
        .expect("write spec");

        let (result, defs) = load_and_dereference(&spec_path, None).expect("dereference");

        // No external refs remain — the mapping value now points inside the document.
        let mut unresolved = collect_unresolved_refs(&result);
        for def_val in defs.values() {
            unresolved.extend(collect_unresolved_refs(def_val));
        }
        assert!(
            unresolved.is_empty(),
            "expected zero unresolved refs after mapping rewrite, got: {unresolved:?}"
        );

        // The mapping value is rewritten to the lifted def pointer.
        let mapping =
            result["components"]["schemas"]["Channel"]["discriminator"]["mapping"]["DEFAULT"]
                .as_str()
                .expect("mapping DEFAULT string");
        assert_eq!(mapping, "#/$defs/BasicChannel");
        assert!(defs.contains_key("BasicChannel"));
    }

    #[test]
    fn discriminator_mapping_target_not_lifted_stays_external_and_fails_guard() {
        // Safety branch: a mapping value whose target is NOT otherwise referenced
        // (no oneOf/$ref lifts it into $defs) must be left external — never pointed
        // at a missing #/$defs key — so collect_unresolved_refs then flags it and
        // the build fails, rather than silently producing a dangling pointer.
        let dir = tempfile::tempdir().expect("tempdir");
        let schemas_dir = dir.path().join("schemas");
        fs::create_dir_all(&schemas_dir).expect("create schemas dir");

        // Note: no BasicChannel.yaml on disk and no oneOf/$ref to it — nothing lifts it.
        let spec_path = schemas_dir.join("openapi.yaml");
        fs::write(
            &spec_path,
            concat!(
                "openapi: '3.0.0'\n",
                "components:\n",
                "  schemas:\n",
                "    Channel:\n",
                "      type: object\n",
                "      discriminator:\n",
                "        propertyName: kind\n",
                "        mapping:\n",
                "          DEFAULT: './BasicChannel.yaml'\n",
            ),
        )
        .expect("write spec");

        let (result, defs) = load_and_dereference(&spec_path, None).expect("dereference");

        // The mapping value is left untouched (no matching def to point at)...
        let mapping =
            result["components"]["schemas"]["Channel"]["discriminator"]["mapping"]["DEFAULT"]
                .as_str()
                .expect("mapping DEFAULT string");
        assert_eq!(mapping, "./BasicChannel.yaml");
        assert!(!defs.contains_key("BasicChannel"));

        // ...and the unresolved-ref guard flags it, so the build would fail.
        let mut unresolved = collect_unresolved_refs(&result);
        for def_val in defs.values() {
            unresolved.extend(collect_unresolved_refs(def_val));
        }
        assert!(
            unresolved.contains(&"./BasicChannel.yaml".to_owned()),
            "expected the un-liftable mapping to be flagged as unresolved, got: {unresolved:?}"
        );
    }

    #[test]
    fn dereference_lifts_external_ref_to_defs() {
        let dir = tempfile::tempdir().expect("tempdir");
        let model_dir = dir.path().join("schemas").join("models");
        fs::create_dir_all(&model_dir).expect("create model dir");

        fs::write(
            model_dir.join("Widget.yaml"),
            "type: object\nproperties:\n  id:\n    type: string\n",
        )
        .expect("write model");

        let spec_path = dir.path().join("schemas").join("openapi.yaml");
        fs::write(
            &spec_path,
            "openapi: '3.0.0'\ncomponents:\n  schemas:\n    Widget:\n      $ref: './models/Widget.yaml'\n",
        )
        .expect("write spec");

        let (_result, defs) = load_and_dereference(&spec_path, None).expect("dereference");
        assert!(
            defs.contains_key("Widget"),
            "Widget should be lifted to $defs"
        );
        let widget = &defs["Widget"];
        assert_eq!(widget.get("type").and_then(|v| v.as_str()), Some("object"));
    }

    #[test]
    fn dereference_deduplicates_same_external_ref() {
        let dir = tempfile::tempdir().expect("tempdir");
        let model_dir = dir.path().join("schemas").join("models");
        fs::create_dir_all(&model_dir).expect("create model dir");

        fs::write(
            model_dir.join("Tag.yaml"),
            "type: object\nproperties:\n  name:\n    type: string\n",
        )
        .expect("write model");

        // Spec references Tag from two different endpoints
        let spec_path = dir.path().join("schemas").join("openapi.yaml");
        fs::write(
            &spec_path,
            r#"openapi: '3.0.0'
paths:
  /a:
    get:
      operationId: getA
      responses:
        '200':
          content:
            application/json:
              schema:
                $ref: './models/Tag.yaml'
  /b:
    get:
      operationId: getB
      responses:
        '200':
          content:
            application/json:
              schema:
                $ref: './models/Tag.yaml'
"#,
        )
        .expect("write spec");

        let (result, defs) = load_and_dereference(&spec_path, None).expect("dereference");
        assert!(defs.contains_key("Tag"), "Tag should be in $defs");

        // Both response schemas should be $ref pointers, not inlined
        let paths = result.get("paths").expect("paths");
        let schema_a =
            paths["/a"]["get"]["responses"]["200"]["content"]["application/json"]["schema"]
                .as_object()
                .expect("schema_a object");
        assert_eq!(
            schema_a.get("$ref").and_then(|v| v.as_str()),
            Some("#/$defs/Tag"),
            "schema_a should be a local $defs ref"
        );
        let schema_b =
            paths["/b"]["get"]["responses"]["200"]["content"]["application/json"]["schema"]
                .as_object()
                .expect("schema_b object");
        assert_eq!(
            schema_b.get("$ref").and_then(|v| v.as_str()),
            Some("#/$defs/Tag"),
            "schema_b should be a local $defs ref"
        );
    }

    #[test]
    fn dereference_lifts_component_schema_ref() {
        let dir = tempfile::tempdir().expect("tempdir");
        let spec_path = dir.path().join("openapi.yaml");
        fs::write(
            &spec_path,
            r#"openapi: '3.0.0'
components:
  schemas:
    Item:
      type: object
      properties:
        id:
          type: string
paths:
  /items:
    get:
      operationId: listItems
      responses:
        '200':
          content:
            application/json:
              schema:
                type: array
                items:
                  $ref: '#/components/schemas/Item'
"#,
        )
        .expect("write spec");

        let (result, defs) = load_and_dereference(&spec_path, None).expect("dereference");

        // Item should be lifted to $defs
        assert!(defs.contains_key("Item"), "Item should be in $defs");

        // The response schema items should be a local $ref
        let schema = result["paths"]["/items"]["get"]["responses"]["200"]["content"]
            ["application/json"]["schema"]
            .as_object()
            .expect("schema object");
        let items_ref = schema["items"]
            .get("$ref")
            .and_then(|v| v.as_str())
            .expect("items.$ref");
        assert_eq!(items_ref, "#/$defs/Item");
    }

    #[test]
    fn dereference_circular_ref_guard() {
        // Schema A references schema B which references schema A — must not infinite-loop
        let dir = tempfile::tempdir().expect("tempdir");
        let spec_path = dir.path().join("openapi.yaml");
        fs::write(
            &spec_path,
            r#"openapi: '3.0.0'
components:
  schemas:
    NodeA:
      type: object
      properties:
        child:
          $ref: '#/components/schemas/NodeB'
    NodeB:
      type: object
      properties:
        parent:
          $ref: '#/components/schemas/NodeA'
"#,
        )
        .expect("write spec");

        // Must complete without stack overflow or panic
        let (_result, defs) = load_and_dereference(&spec_path, None).expect("dereference");
        assert!(defs.contains_key("NodeA") || defs.contains_key("NodeB"));
    }

    #[test]
    fn derive_defs_key_for_path_extracts_stem() {
        assert_eq!(derive_defs_key_for_path("./models/Order.yaml"), "Order");
        assert_eq!(
            derive_defs_key_for_path("../common-types/error.json"),
            "error"
        );
        assert_eq!(
            derive_defs_key_for_path("https://schemas.api.godaddy.com/v1/json/address.json"),
            "address"
        );
        assert_eq!(
            derive_defs_key_for_path("./Bulk_Ingestion.yaml"),
            "Bulk_Ingestion"
        );
        // Fragment with component name takes precedence over file stem
        assert_eq!(
            derive_defs_key_for_path("./common.yaml#/components/schemas/Foo"),
            "Foo"
        );
        assert_eq!(
            derive_defs_key_for_path("./shared.yaml#/definitions/Bar"),
            "Bar"
        );
        // A `#/properties/<name>` fragment picks out one property's schema, not the
        // file's root schema — it must be folded into the key so two refs into the
        // same file for different properties don't collide (DEVEX-965).
        assert_eq!(
            derive_defs_key_for_path("./models/FulfillmentPlan.yaml#/properties/storeId"),
            "FulfillmentPlan_storeId"
        );
        assert_eq!(
            derive_defs_key_for_path("./models/FulfillmentPlan.yaml#/properties/orderId"),
            "FulfillmentPlan_orderId"
        );
        assert_eq!(
            derive_defs_key_for_path("./models/Foo.yaml#/properties/a/properties/b"),
            "Foo_a_b"
        );
        // A literal property named `a_b` must not collide with nested segments `a`
        // then `b` — both would sanitize to the same "a_b" suffix without escaping
        // the literal underscore first.
        assert_ne!(
            derive_defs_key_for_path("./models/Foo.yaml#/properties/a_b"),
            derive_defs_key_for_path("./models/Foo.yaml#/properties/a/properties/b")
        );
    }
}
