//! Filesystem lookups used when resolving external `$ref` targets.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::github::parse_yaml_or_json;

pub(crate) fn specification_root(spec_dir: &Path) -> Option<&Path> {
    let mut current = Some(spec_dir);
    while let Some(path) = current {
        if path.join("schemas").is_dir() {
            return Some(path);
        }
        current = path.parent();
    }
    None
}

/// Rewrite an absolute-URL `$ref` that names a schema by its `$id` (e.g. the dated
/// `https://godaddy.com/schemas/ucp/<date>/...` URLs) into a reference to the file
/// that declares it, keeping any fragment. The URL does not mirror the file layout,
/// so the declaring file has to be found. Returns `None` for any other ref.
pub(crate) fn id_ref_to_file_ref(ref_str: &str, spec_dir: &Path) -> Option<String> {
    if !ref_str.starts_with("https://") {
        return None;
    }
    let (id, fragment) = match ref_str.split_once('#') {
        Some((id, fragment)) => (id, Some(fragment)),
        None => (ref_str, None),
    };
    let file = find_schema_by_id(&specification_root(spec_dir)?.join("schemas"), id)?;
    Some(match fragment {
        Some(fragment) => format!("{}#{fragment}", file.display()),
        None => file.display().to_string(),
    })
}

/// Find the JSON/YAML file under `dir` whose top-level `$id` is exactly `id`.
fn find_schema_by_id(dir: &Path, id: &str) -> Option<PathBuf> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .collect();
    // Deterministic result if two files ever claim the same `$id`.
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        // `file_type` does not follow symlinks, so a link cannot cause a cycle.
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            if let Some(found) = find_schema_by_id(&path, id) {
                return Some(found);
            }
            continue;
        }
        if !file_type.is_file()
            || !matches!(
                path.extension().and_then(|e| e.to_str()),
                Some("json" | "yaml" | "yml")
            )
        {
            continue;
        }
        let Ok(src) = std::fs::read_to_string(&path) else {
            continue;
        };
        // Cheap prefilter before parsing.
        if !src.contains(id) {
            continue;
        }
        if let Ok(parsed) = parse_yaml_or_json(&src, &path)
            && parsed.get("$id").and_then(Value::as_str) == Some(id)
        {
            return Some(path);
        }
    }
    None
}

/// Try to find `path` in its parent directory with a case-insensitive filename match.
/// Returns the first candidate whose lowercased name equals the lowercased target,
/// or `None` if no unique match is found.
pub(crate) fn resolve_case_insensitive(path: &Path) -> Option<PathBuf> {
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
pub(crate) fn normalize_path(path: &Path) -> PathBuf {
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
pub(crate) fn resolve_common_types_path(
    path: &Path,
    common_types_dir: Option<&Path>,
) -> Option<PathBuf> {
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

/// Whether another schema file under `dir` has the same file name as `file` but a
/// different top-level `$id`. `$defs` keys follow file names, so such files would
/// otherwise share one definition. Files without an `$id` are never reported:
/// same-named copies (e.g. a spec's `json/` and `yaml/` variants) are not known to differ.
pub(crate) fn has_conflicting_namesake(dir: &Path, file: &Path) -> bool {
    fn schema_id(path: &Path) -> Option<String> {
        let src = std::fs::read_to_string(path).ok()?;
        let parsed = parse_yaml_or_json(&src, path).ok()?;
        parsed.get("$id")?.as_str().map(str::to_owned)
    }
    fn any_conflict(dir: &Path, file: &Path, id: &str) -> bool {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return false;
        };
        entries.filter_map(|e| e.ok()).any(|entry| {
            let path = entry.path();
            match entry.file_type() {
                Ok(t) if t.is_dir() => any_conflict(&path, file, id),
                Ok(t)
                    if t.is_file() && entry.file_name() == file.file_name().unwrap_or_default() =>
                {
                    schema_id(&path).is_some_and(|other| other != id)
                }
                _ => false,
            }
        })
    }
    schema_id(file).is_some_and(|id| any_conflict(dir, file, &id))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use crate::dereference::{
        collect_unresolved_refs, load_and_dereference,
        load_and_dereference_distinguishing_namesakes,
    };

    #[test]
    fn dereference_resolves_absolute_url_ref_by_schema_id() {
        // The URL does not mirror the file layout: the file is found by its `$id`.
        let dir = tempfile::tempdir().expect("tempdir");
        let schemas_dir = dir.path().join("schemas");
        let model_dir = schemas_dir.join("models");
        fs::create_dir_all(&model_dir).expect("create model dir");

        let id = "https://example.com/schemas/2026-01-01/shopping/widget.json";
        fs::write(
            model_dir.join("widget.schema.json"),
            format!(
                r#"{{"$id": "{id}", "$defs": {{"size": {{"type": "integer"}}}}, "type": "object"}}"#
            ),
        )
        .expect("write model");

        let spec_path = schemas_dir.join("openapi.yaml");
        fs::write(
            &spec_path,
            format!(
                "openapi: '3.0.0'\ncomponents:\n  schemas:\n    Widget:\n      $ref: '{id}'\n    Size:\n      $ref: '{id}#/$defs/size'\n"
            ),
        )
        .expect("write spec");

        let (result, defs) = load_and_dereference(&spec_path, None).expect("dereference");
        let unresolved = collect_unresolved_refs(&result);
        assert!(
            unresolved.is_empty(),
            "expected zero unresolved $refs, got: {unresolved:?}"
        );
        assert_eq!(defs["widget_schema"]["type"], "object");
        assert_eq!(defs["widget_schema_size"]["type"], "integer");
    }

    #[test]
    fn dereference_keeps_same_named_files_in_different_directories_apart() {
        let dir = tempfile::tempdir().expect("tempdir");
        let schemas_dir = dir.path().join("schemas");
        for (sub, field) in [("product", "category"), ("variant", "default")] {
            let sub_dir = schemas_dir.join("models").join(sub);
            fs::create_dir_all(&sub_dir).expect("create dir");
            fs::write(
                sub_dir.join("base.schema.json"),
                format!(
                    r#"{{"$id": "https://example.com/{sub}/base.json", "type": "object", "properties": {{"{field}": {{"type": "string"}}}}}}"#
                ),
            )
            .expect("write model");
        }
        let spec_path = schemas_dir.join("openapi.yaml");
        fs::write(
            &spec_path,
            "openapi: '3.0.0'\ncomponents:\n  schemas:\n    P:\n      $ref: 'https://example.com/product/base.json'\n    V:\n      $ref: 'https://example.com/variant/base.json'\n",
        )
        .expect("write spec");

        let (_result, defs) =
            load_and_dereference_distinguishing_namesakes(&spec_path, None).expect("dereference");
        assert!(defs["product_base_schema"]["properties"]["category"].is_object());
        assert!(defs["variant_base_schema"]["properties"]["default"].is_object());

        // The default entry point keeps file-name keys, so specs already generated
        // that way are not renamed.
        let (_result, defs) = load_and_dereference(&spec_path, None).expect("dereference");
        assert!(defs.contains_key("base_schema"));
        assert!(!defs.contains_key("product_base_schema"));
    }

    #[test]
    fn dereference_leaves_absolute_url_ref_without_matching_id_unresolved() {
        let dir = tempfile::tempdir().expect("tempdir");
        let schemas_dir = dir.path().join("schemas");
        fs::create_dir_all(&schemas_dir).expect("create schemas dir");
        let spec_path = schemas_dir.join("openapi.yaml");
        fs::write(
            &spec_path,
            "openapi: '3.0.0'\ncomponents:\n  schemas:\n    Widget:\n      $ref: 'https://example.com/missing.json'\n",
        )
        .expect("write spec");

        let (result, _defs) = load_and_dereference(&spec_path, None).expect("dereference");
        assert_eq!(
            collect_unresolved_refs(&result),
            vec!["https://example.com/missing.json".to_owned()]
        );
    }
}
