mod dereference;
mod domains_merge;
mod github;
mod graphql;
mod hosting_spec;
mod manifest;
mod openapi;

use std::{
    collections::{BTreeMap, HashSet},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use indexmap::IndexMap;
use serde::Serialize;
use serde_json::Value;

use dereference::{collect_unresolved_refs, load_and_dereference};
use github::{discover_spec_sources, local_spec_sources};
use graphql::synthesize_graphql_domain;
use manifest::load_source_manifest;
use openapi::process_spec;

// ---------------------------------------------------------------------------
// Output manifest types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct ManifestEntry {
    file: String,
    title: String,
    #[serde(rename = "endpointCount")]
    endpoint_count: usize,
}

#[derive(Debug, Serialize)]
struct CatalogManifest {
    generated: String,
    domains: BTreeMap<String, ManifestEntry>,
}

// ---------------------------------------------------------------------------
// Stale file removal
// ---------------------------------------------------------------------------

fn remove_stale_json(output_dir: &Path, active: &HashSet<String>) -> Result<()> {
    for entry in std::fs::read_dir(output_dir).context("failed to read output dir")? {
        let entry = entry.context("dir entry error")?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == "manifest.json" || !name.ends_with(".json") {
            continue;
        }
        if !active.contains(&name) {
            std::fs::remove_file(entry.path())
                .with_context(|| format!("failed to remove stale {name}"))?;
            eprintln!("Removed stale: {name}");
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Output directory resolution
// ---------------------------------------------------------------------------

fn resolve_output_dir() -> PathBuf {
    // CARGO_MANIFEST_DIR resolves to rust/tools/generate-api-catalog at build time
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir.join("../../schemas/api")
}

fn resolve_hosting_spec_path() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir.join("../../schemas/openapi/hosting-nodejs-public-v1.yaml")
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() -> Result<()> {
    let output_dir = resolve_output_dir();
    std::fs::create_dir_all(&output_dir).context("failed to create output dir")?;

    hosting_spec::refresh(&resolve_hosting_spec_path())?;

    let source_manifest = load_source_manifest()?;
    eprintln!("Discovering specification repositories...");
    let (mut sources, tmpdir) = discover_spec_sources(&source_manifest)?;

    // `domains` is a normal remote source (cloned like any commerce/location
    // repo), but its v3 OpenAPI doc is *also* progenitor's codegen input for
    // the domains-client crate once merged with the one v1 operation v3
    // doesn't yet serve. Reuse this same clone rather than fetching it twice.
    if let Some(domains_source) = sources.iter().find(|s| s.domain == "domains") {
        domains_merge::refresh(
            &domains_source.spec_file,
            &domains_merge::domains_client_oas3_path(),
        )
        .context("failed to refresh domains-client codegen spec")?;
    }

    sources.extend(local_spec_sources(&source_manifest)?);

    if sources.is_empty() {
        bail!("no specification repositories discovered — refusing to overwrite catalog output");
    }

    let ct_dir = tmpdir.join("__common-types");
    let common_types: Option<&Path> = if ct_dir.exists() { Some(&ct_dir) } else { None };

    let mut manifest = CatalogManifest {
        generated: chrono::Utc::now().to_rfc3339(),
        domains: BTreeMap::new(),
    };
    let mut active_files: HashSet<String> = HashSet::new();
    let mut total_endpoints = 0usize;

    for source in &sources {
        eprintln!(
            "Processing {} ({}/{})...",
            source.domain, source.repo_name, source.spec_version
        );

        let (catalog, defs) = if source.graphql_only {
            (
                synthesize_graphql_domain(source, common_types),
                IndexMap::new(),
            )
        } else {
            let (spec, defs) = load_and_dereference(&source.spec_file, common_types)
                .with_context(|| format!("failed to dereference {}", source.repo_name))?;
            let mut unresolved = collect_unresolved_refs(&spec);
            for def_val in defs.values() {
                unresolved.extend(collect_unresolved_refs(def_val));
            }
            unresolved.sort();
            unresolved.dedup();
            if !unresolved.is_empty() {
                bail!(
                    "{} unresolved $ref(s) remain in {} after dereferencing:\n  {}",
                    unresolved.len(),
                    source.repo_name,
                    unresolved.join("\n  ")
                );
            }
            (
                process_spec(spec, &source.domain, &source.spec_file, common_types),
                defs,
            )
        };

        let filename = format!("{}.json", source.domain);
        let out_path = output_dir.join(&filename);
        let mut catalog_value =
            serde_json::to_value(&catalog).context("failed to convert catalog to value")?;
        if !defs.is_empty() {
            let defs_map: serde_json::Map<String, Value> = defs.into_iter().collect();
            catalog_value["$defs"] = Value::Object(defs_map);
        }
        let json = serde_json::to_string_pretty(&catalog_value)
            .context("failed to serialize catalog domain")?;
        std::fs::write(&out_path, &json)
            .with_context(|| format!("failed to write {}", out_path.display()))?;

        let ep_count = catalog.endpoints.len();
        eprintln!(
            "  {} endpoints from {} v{}",
            ep_count, catalog.title, catalog.version
        );
        total_endpoints += ep_count;

        manifest.domains.insert(
            source.domain.clone(),
            ManifestEntry {
                file: filename.clone(),
                title: catalog.title,
                endpoint_count: ep_count,
            },
        );
        active_files.insert(filename);
    }

    // Remove stale *.json files
    remove_stale_json(&output_dir, &active_files)?;

    // Write manifest
    let manifest_path = output_dir.join("manifest.json");
    let manifest_json =
        serde_json::to_string_pretty(&manifest).context("failed to serialize manifest")?;
    std::fs::write(&manifest_path, &manifest_json)
        .with_context(|| format!("failed to write {}", manifest_path.display()))?;

    // Cleanup temp dir
    if let Err(e) = std::fs::remove_dir_all(&tmpdir) {
        eprintln!(
            "WARNING: failed to clean up temp dir {}: {e}",
            tmpdir.display()
        );
    }

    eprintln!(
        "\nGenerated API catalog: {} domains, {total_endpoints} endpoints",
        manifest.domains.len()
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Drift guard: the committed catalog files and `manifest.json` must match the
    /// intentional source-manifest contract, and every domain's endpoint count must
    /// agree between its file and the manifest. Catches accidental drift (a domain
    /// added/removed, a hand-edited catalog, a stale manifest) on every `cargo test`
    /// run — no network or credentials required.
    #[test]
    fn committed_catalog_matches_expected_domains_and_manifest() {
        let dir = resolve_output_dir();
        let source_manifest = load_source_manifest().expect("load source manifest");

        // Domain files present on disk (excluding manifest.json).
        let mut files: Vec<String> = std::fs::read_dir(&dir)
            .expect("read schemas/api dir")
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let n = e.file_name().to_string_lossy().into_owned();
                match n.strip_suffix(".json") {
                    Some(stem) if n != "manifest.json" => Some(stem.to_owned()),
                    _ => None,
                }
            })
            .collect();
        files.sort();

        let expected = source_manifest.expected_domains();

        assert_eq!(
            files, expected,
            "catalog domain files drifted from api-catalog-sources.json"
        );

        // manifest.json must list exactly the same domains...
        let manifest: Value = serde_json::from_str(
            &std::fs::read_to_string(dir.join("manifest.json")).expect("read manifest.json"),
        )
        .expect("parse manifest.json");
        let domains = manifest["domains"]
            .as_object()
            .expect("manifest.domains is an object");

        let mut manifest_domains: Vec<String> = domains.keys().cloned().collect();
        manifest_domains.sort();
        assert_eq!(
            manifest_domains, expected,
            "manifest.json domains differ from api-catalog-sources.json / catalog files"
        );

        // ...and each domain's endpoint count must match between file and manifest.
        for domain in &expected {
            let catalog: Value = serde_json::from_str(
                &std::fs::read_to_string(dir.join(format!("{domain}.json")))
                    .unwrap_or_else(|e| panic!("read {domain}.json: {e}")),
            )
            .unwrap_or_else(|e| panic!("parse {domain}.json: {e}"));

            // Fail loudly on a structurally broken file rather than comparing two
            // silent zeros (which would hide a missing endpoints array / count).
            let actual = catalog["endpoints"]
                .as_array()
                .unwrap_or_else(|| panic!("'{domain}.json' has no endpoints array"))
                .len();
            let manifest_count = domains[domain]["endpointCount"]
                .as_u64()
                .unwrap_or_else(|| panic!("manifest.json missing endpointCount for '{domain}'"))
                as usize;
            assert_eq!(
                actual, manifest_count,
                "endpoint-count drift for '{domain}': file has {actual}, manifest says {manifest_count}"
            );
        }
    }
}
