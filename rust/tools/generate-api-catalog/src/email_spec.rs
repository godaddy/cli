//! Refreshes the vendored Email OpenAPI document used by `email-client`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::{Map, Value};

const HOST: &str = "https://api.godaddy.com";
const PATH_PREFIX: &str = "/v1/email";

pub(crate) fn email_client_oas3_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../email-client/openapi/email.oas3.json")
}

pub(crate) fn refresh(
    spec_path: &Path,
    common_types: Option<&Path>,
    out_path: &Path,
) -> Result<()> {
    let (mut spec, defs) = crate::dereference::load_and_dereference(spec_path, common_types)
        .with_context(|| format!("failed to dereference Email spec {}", spec_path.display()))?;

    let object = spec
        .as_object_mut()
        .context("Email spec is not a JSON object")?;
    let schemas = object
        .entry("components")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .context("Email spec components is not an object")?
        .entry("schemas")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .context("Email spec components.schemas is not an object")?;
    for (name, definition) in defs {
        schemas.insert(name, definition);
    }
    crate::spec_normalize::rewrite_defs_refs(&mut spec);
    crate::spec_normalize::remove_remaining_relative_refs(&mut spec);
    crate::spec_normalize::strip_external_formats(&mut spec, false);
    collapse_alias_refs(&mut spec);
    crate::spec_normalize::remove_self_referencing_schemas(&mut spec);

    let object = spec
        .as_object_mut()
        .context("Email spec is not a JSON object")?;
    object.insert("openapi".to_owned(), Value::String("3.0.3".to_owned()));
    object.insert(
        "servers".to_owned(),
        serde_json::json!([{ "url": HOST, "description": "Email API host" }]),
    );
    crate::spec_normalize::prefix_paths(&mut spec, PATH_PREFIX)?;

    let json = serde_json::to_string_pretty(&spec).context("serialize Email codegen spec")?;
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    std::fs::write(out_path, json).with_context(|| format!("write {}", out_path.display()))?;
    eprintln!("   wrote {}", out_path.display());
    Ok(())
}

/// The source spec names common-types schemas twice: once as a friendly
/// top-level alias (`Error: {$ref: ./common-types/…/error.yaml}`) and once —
/// because `dereference::load_and_dereference` lifts multiply-referenced
/// external (submodule) schemas into `$defs` rather than inlining them — as
/// the lifted def itself, keyed by the source filename (`error`). Both
/// PascalCase to the same generated Rust type name (`Error`), which
/// progenitor emits as two conflicting struct definitions. Collapse each pure
/// single-key `{"$ref": "#/components/schemas/<target>"}` alias by inlining
/// the target's definition into the alias's slot, dropping the target, and
/// rewriting any other refs that pointed at the target directly.
fn collapse_alias_refs(spec: &mut Value) {
    loop {
        let Some(schemas) = spec
            .pointer("/components/schemas")
            .and_then(Value::as_object)
        else {
            return;
        };
        let found = schemas.iter().find_map(|(name, def)| {
            let obj = def.as_object()?;
            if obj.len() != 1 {
                return None;
            }
            let reference = obj.get("$ref")?.as_str()?;
            let target = reference.strip_prefix("#/components/schemas/")?;
            if target == name {
                return None;
            }
            Some((name.clone(), target.to_owned()))
        });
        let Some((alias, target)) = found else {
            return;
        };
        let schemas = spec
            .pointer_mut("/components/schemas")
            .and_then(Value::as_object_mut)
            .expect("schemas object present (checked above)");
        let Some(target_def) = schemas.get(&target).cloned() else {
            return;
        };
        schemas.insert(alias.clone(), target_def);
        schemas.remove(&target);
        rewrite_ref_target(spec, &target, &alias);
    }
}

fn rewrite_ref_target(value: &mut Value, from: &str, to: &str) {
    let from_ref = format!("#/components/schemas/{from}");
    match value {
        Value::Object(map) => {
            if let Some(Value::String(reference)) = map.get("$ref")
                && reference == &from_ref
            {
                map.insert(
                    "$ref".to_owned(),
                    Value::String(format!("#/components/schemas/{to}")),
                );
            }
            for child in map.values_mut() {
                rewrite_ref_target(child, from, to);
            }
        }
        Value::Array(values) => {
            for value in values {
                rewrite_ref_target(value, from, to);
            }
        }
        _ => {}
    }
}
