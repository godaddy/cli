use std::collections::HashSet;
use std::path::Path;

use anyhow::{Context, Result};
use indexmap::IndexMap;
use serde_json::Value;

use crate::github::parse_yaml_or_json;
use crate::ref_paths::{
    has_conflicting_namesake, id_ref_to_file_ref, locate_ref_file, normalize_path,
    specification_root,
};
use crate::visibility;

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

    // A fragment selecting one schema from a multi-schema document needs its
    // own key. Without it, every UCP ref through `ucp-refs.schema.json` would
    // collide under one definition and silently produce incorrect generated types.
    match frag.and_then(|f| f.strip_prefix("#/$defs/")) {
        Some(name) => sanitize_defs_key(&format!("{base_key}_{name}")),
        None => match frag.and_then(|f| f.strip_prefix("#/properties/")) {
            Some(rest) => {
                let suffix = rest
                    .split("/properties/")
                    .map(|seg| seg.replace('_', "__"))
                    .collect::<Vec<_>>()
                    .join("_");
                sanitize_defs_key(&format!("{base_key}_{suffix}"))
            }
            None => base_key,
        },
    }
}

/// Prefix `key` with the file's parent directory when another schema file in
/// the specification has the same name but a different `$id` (e.g.
/// `product-metadata/base.schema.json` and
/// `variant-metadata/base.schema.json`); otherwise they would share one `$defs`
/// entry and whichever was seen first would silently win.
fn qualify_ambiguous_key(key: String, ref_str: &str, spec_dir: &Path) -> String {
    let file = normalize_path(&spec_dir.join(ref_str.split('#').next().unwrap_or(ref_str)));
    let file = file.as_path();
    let qualifier = file.parent().and_then(Path::file_name);
    match (qualifier, specification_root(spec_dir)) {
        (Some(qualifier), Some(root)) if has_conflicting_namesake(&root.join("schemas"), file) => {
            sanitize_defs_key(&format!("{}_{key}", qualifier.to_string_lossy()))
        }
        _ => key,
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

/// Builds the `{"$ref": "#/$defs/<key>"}` that replaces a lifted `$ref`,
/// keeping the keywords that sat beside it. JSON Schema 2020-12 applies
/// sibling keywords alongside a `$ref`, so dropping them loses constraints
/// (e.g. `positive-money` refines `simple-money` with `value.minimum: 0`) and
/// per-use annotations such as `description` and `readOnly`.
#[expect(
    clippy::too_many_arguments,
    reason = "threads the same resolution context as `dereference`"
)]
fn lifted_ref_with_siblings(
    key: &str,
    original: &serde_json::Map<String, Value>,
    root: &Value,
    spec_dir: &Path,
    common_types_dir: Option<&Path>,
    qualify_namesakes: bool,
    defs: &mut IndexMap<String, Value>,
    depth: usize,
) -> Value {
    let mut lifted = serde_json::Map::new();
    lifted.insert(
        "$ref".to_owned(),
        Value::String(format!("#/$defs/{}", json_pointer_escape(key))),
    );
    let siblings: serde_json::Map<String, Value> = original
        .iter()
        .filter(|(name, _)| name.as_str() != "$ref")
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect();
    if !siblings.is_empty()
        && let Value::Object(siblings) = dereference(
            root,
            Value::Object(siblings),
            spec_dir,
            common_types_dir,
            qualify_namesakes,
            defs,
            depth + 1,
        )
    {
        lifted.extend(siblings);
    }
    Value::Object(lifted)
}

fn dereference(
    root: &Value,
    current: Value,
    spec_dir: &Path,
    common_types_dir: Option<&Path>,
    qualify_namesakes: bool,
    defs: &mut IndexMap<String, Value>,
    depth: usize,
) -> Value {
    // Backstop only: circular refs are caught by the `defs` placeholder below. Depth
    // counts structural nesting plus every external file hop, and real spec chains
    // (e.g. UCP schemas routed through a shared refs document) run past 90.
    if depth > 256 {
        return current;
    }
    match current {
        Value::Object(mut map) => {
            if let Some(Value::String(ref_str)) = map.get("$ref").cloned() {
                // Key and resolve `$id` URLs by the file that declares them, so lifted
                // `$defs` keys follow file names rather than generic URL tails.
                let ref_str = id_ref_to_file_ref(&ref_str, spec_dir).unwrap_or(ref_str);
                if let Some(key) = should_lift_to_defs(&ref_str) {
                    let key = if qualify_namesakes && !ref_str.starts_with('#') {
                        qualify_ambiguous_key(key, &ref_str, spec_dir)
                    } else {
                        key
                    };
                    // Already registered — dedup, unless the target is a private file
                    if defs.contains_key(&key) {
                        if visibility::ref_targets_private_file(&ref_str, spec_dir) {
                            return Value::Object(map);
                        }
                        return lifted_ref_with_siblings(
                            &key,
                            &map,
                            root,
                            spec_dir,
                            common_types_dir,
                            qualify_namesakes,
                            defs,
                            depth,
                        );
                    }
                    // Insert null placeholder before resolving (circular ref guard)
                    defs.insert(key.clone(), Value::Null);
                    let resolved_opt = if ref_str.starts_with('#') {
                        // Local component ref: resolve from this spec's root
                        resolve_local_ref(root, &ref_str).cloned().map(|v| {
                            dereference(
                                root,
                                v,
                                spec_dir,
                                common_types_dir,
                                qualify_namesakes,
                                defs,
                                depth + 1,
                            )
                        })
                    } else {
                        // External file/URL ref
                        resolve_ref(
                            root,
                            &ref_str,
                            spec_dir,
                            common_types_dir,
                            qualify_namesakes,
                            defs,
                            depth + 1,
                        )
                    };
                    if let Some(content) = resolved_opt {
                        defs.insert(key.clone(), content);
                        return lifted_ref_with_siblings(
                            &key,
                            &map,
                            root,
                            spec_dir,
                            common_types_dir,
                            qualify_namesakes,
                            defs,
                            depth,
                        );
                    }
                    // Resolution failed — remove placeholder and preserve original $ref
                    // so collect_unresolved_refs() will catch it.
                    defs.shift_remove(&key);
                    return Value::Object(map);
                }
                // Non-liftable local ref (e.g. #/paths/...) — inline as before
                let resolved = resolve_ref(
                    root,
                    &ref_str,
                    spec_dir,
                    common_types_dir,
                    qualify_namesakes,
                    defs,
                    depth + 1,
                );
                return resolved
                    .map(|v| {
                        dereference(
                            root,
                            v,
                            spec_dir,
                            common_types_dir,
                            qualify_namesakes,
                            defs,
                            depth + 1,
                        )
                    })
                    .unwrap_or(Value::Object(map));
            }
            // Recurse into all values
            for v in map.values_mut() {
                *v = dereference(
                    root,
                    v.take(),
                    spec_dir,
                    common_types_dir,
                    qualify_namesakes,
                    defs,
                    depth + 1,
                );
            }
            Value::Object(map)
        }
        Value::Array(mut arr) => {
            for v in &mut arr {
                *v = dereference(
                    root,
                    v.take(),
                    spec_dir,
                    common_types_dir,
                    qualify_namesakes,
                    defs,
                    depth + 1,
                );
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
    qualify_namesakes: bool,
    defs: &mut IndexMap<String, Value>,
    depth: usize,
) -> Option<Value> {
    if ref_str.starts_with("#/") {
        return resolve_local_ref(root, ref_str).cloned();
    }

    if ref_str == "https://json-schema.org/draft/2020-12/schema" {
        return Some(serde_json::json!(true));
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
        return load_external_ref(
            &local_path,
            common_types_dir,
            qualify_namesakes,
            defs,
            depth,
        );
    }

    if let Some(url_path) = ref_str.strip_prefix("https://godaddy.com/ucp/schemas/") {
        let (file_part, fragment) = split_external_ref(url_path);
        let local_path = specification_root(spec_dir)?
            .join("schemas")
            .join(file_part);
        return resolve_external_fragment(
            &local_path,
            fragment,
            common_types_dir,
            qualify_namesakes,
            defs,
            depth,
        );
    }

    if let Some(url_path) = ref_str.strip_prefix("https://ucp.dev/schemas/shopping/") {
        let (file_part, fragment) = split_external_ref(url_path);
        let local_path = specification_root(spec_dir)?
            .join("schemas/ucp/shopping")
            .join(file_part);
        return resolve_external_fragment(
            &local_path,
            fragment,
            common_types_dir,
            qualify_namesakes,
            defs,
            depth,
        );
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
    // Only a file that can't be located falls back, never one found and rejected.
    let external_root = match locate_ref_file(&file_path, common_types_dir) {
        Some(found) => load_ref_file(&found, common_types_dir, qualify_namesakes, defs, depth),
        None => spec_dir.parent().and_then(|parent| {
            load_external_ref(
                &parent.join(file_part),
                common_types_dir,
                qualify_namesakes,
                defs,
                depth,
            )
        }),
    }?;

    if let Some(frag) = fragment
        && !frag.is_empty()
    {
        return resolve_local_ref(&external_root, frag).cloned();
    }
    Some(external_root)
}

fn split_external_ref(reference: &str) -> (&str, Option<&str>) {
    match reference.find('#') {
        Some(index) => (&reference[..index], Some(&reference[index..])),
        None => (reference, None),
    }
}

fn resolve_external_fragment(
    path: &Path,
    fragment: Option<&str>,
    common_types_dir: Option<&Path>,
    qualify_namesakes: bool,
    defs: &mut IndexMap<String, Value>,
    depth: usize,
) -> Option<Value> {
    let root = load_external_ref(path, common_types_dir, qualify_namesakes, defs, depth)?;
    fragment.map_or(Some(root.clone()), |fragment| {
        resolve_local_ref(&root, fragment).cloned()
    })
}

fn load_external_ref(
    path: &Path,
    common_types_dir: Option<&Path>,
    qualify_namesakes: bool,
    defs: &mut IndexMap<String, Value>,
    depth: usize,
) -> Option<Value> {
    let resolved = locate_ref_file(path, common_types_dir)?;
    load_ref_file(&resolved, common_types_dir, qualify_namesakes, defs, depth)
}

fn load_ref_file(
    resolved: &Path,
    common_types_dir: Option<&Path>,
    qualify_namesakes: bool,
    defs: &mut IndexMap<String, Value>,
    depth: usize,
) -> Option<Value> {
    let src = std::fs::read_to_string(resolved)
        .map_err(|e| eprintln!("WARNING: cannot read ref file {}: {e}", resolved.display()))
        .ok()?;
    let mut parsed = parse_yaml_or_json(&src, resolved)
        .map_err(|e| eprintln!("WARNING: cannot parse ref file {}: {e}", resolved.display()))
        .ok()?;
    visibility::prune_ref_file(resolved, &mut parsed)?;
    let dir = resolved.parent().unwrap_or(resolved);
    Some(dereference(
        &parsed.clone(),
        parsed,
        dir,
        common_types_dir,
        qualify_namesakes,
        defs,
        depth + 1,
    ))
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
    load_and_dereference_with(spec_file, common_types_dir, false)
}

/// Like [`load_and_dereference`], but lifts schema files that share a file name yet
/// declare different `$id`s under distinct `$defs` keys instead of letting one
/// silently replace the other. Opt-in so specs already generated keep their keys.
pub(crate) fn load_and_dereference_distinguishing_namesakes(
    spec_file: &Path,
    common_types_dir: Option<&Path>,
) -> Result<(Value, IndexMap<String, Value>)> {
    load_and_dereference_with(spec_file, common_types_dir, true)
}

fn load_and_dereference_with(
    spec_file: &Path,
    common_types_dir: Option<&Path>,
    qualify_namesakes: bool,
) -> Result<(Value, IndexMap<String, Value>)> {
    let src = std::fs::read_to_string(spec_file)
        .with_context(|| format!("failed to read {}", spec_file.display()))?;
    let mut parsed = parse_yaml_or_json(&src, spec_file)?;
    visibility::prune_spec(spec_file, &mut parsed)?;
    let spec_dir = spec_file.parent().unwrap_or(spec_file);
    let mut defs = IndexMap::new();
    let mut dereffed = dereference(
        &parsed.clone(),
        parsed,
        spec_dir,
        common_types_dir,
        qualify_namesakes,
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
    visibility::drop_emptied_paths_and_tags(&mut dereffed);
    Ok((dereffed, defs))
}

#[cfg(test)]
#[path = "dereference_tests.rs"]
mod tests;
