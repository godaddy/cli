use std::collections::HashSet;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

const SOURCE_MANIFEST_JSON: &str = include_str!("../../../api-catalog-sources.json");

// ---------------------------------------------------------------------------
// Source manifest types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub(crate) struct RemoteCatalogSource {
    pub(crate) domain: String,
    pub(crate) repository: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct LocalCatalogSource {
    pub(crate) domain: String,
    pub(crate) path: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CatalogSourceManifest {
    version: u32,
    pub(crate) remote: Vec<RemoteCatalogSource>,
    pub(crate) local: Vec<LocalCatalogSource>,
}

impl CatalogSourceManifest {
    pub(crate) fn expected_domains(&self) -> Vec<String> {
        let mut domains: Vec<String> = self
            .remote
            .iter()
            .map(|source| source.domain.clone())
            .chain(self.local.iter().map(|source| source.domain.clone()))
            .collect();
        domains.sort();
        domains
    }
}

pub(crate) fn load_source_manifest() -> Result<CatalogSourceManifest> {
    let manifest: CatalogSourceManifest = serde_json::from_str(SOURCE_MANIFEST_JSON)
        .context("failed to parse api-catalog-sources.json")?;

    if manifest.version != 1 {
        bail!(
            "unsupported api-catalog-sources.json version {}",
            manifest.version
        );
    }
    let expected = manifest.expected_domains();
    let unique: HashSet<&str> = expected.iter().map(String::as_str).collect();
    if unique.len() != expected.len() {
        bail!("api-catalog-sources.json contains duplicate domain names");
    }

    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_manifest_defines_the_reconciled_catalog_domains() {
        let manifest = load_source_manifest().expect("load source manifest");
        let expected = manifest.expected_domains();

        assert_eq!(expected.len(), 22);
        assert_eq!(manifest.remote.len(), 21);
        assert_eq!(
            manifest
                .local
                .iter()
                .map(|source| source.domain.as_str())
                .collect::<Vec<_>>(),
            ["hosting-nodejs"]
        );
    }
}
