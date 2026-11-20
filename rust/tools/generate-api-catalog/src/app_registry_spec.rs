//! Refreshes the vendored App Registry GraphQL schema used by
//! `platform-app-client`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::graphql_sdl;

pub(crate) fn app_registry_client_schema_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../platform-app-client/graphql/schema.graphql")
}

pub(crate) fn refresh(spec_path: &Path, out_path: &Path) -> Result<()> {
    let sdl = std::fs::read_to_string(spec_path)
        .with_context(|| format!("failed to read App Registry schema {}", spec_path.display()))?;

    graphql_sdl::ensure_query_root(&sdl, "App Registry")?;

    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    std::fs::write(out_path, &sdl).with_context(|| format!("write {}", out_path.display()))?;
    eprintln!("   wrote {}", out_path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refresh_leaves_the_vendored_schema_alone_when_validation_fails() {
        let dir = tempfile::tempdir().expect("tempdir");
        let spec = dir.path().join("spec.graphql");
        let out = dir.path().join("out/schema.graphql");
        std::fs::create_dir_all(out.parent().expect("parent")).expect("mkdir");
        std::fs::write(&out, "type Query { existing: String }").expect("seed");
        std::fs::write(&spec, "type QueryApplicationsConnection { total: Int }").expect("spec");

        refresh(&spec, &out).expect_err("invalid schema is rejected");

        assert_eq!(
            std::fs::read_to_string(&out).expect("read"),
            "type Query { existing: String }",
            "the previously vendored schema must not be overwritten"
        );
    }
}
