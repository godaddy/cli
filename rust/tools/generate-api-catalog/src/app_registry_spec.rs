//! Refreshes the vendored App Registry GraphQL schema used by
//! `platform-app-client`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

pub(crate) fn app_registry_client_schema_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../platform-app-client/graphql/schema.graphql")
}

pub(crate) fn refresh(spec_path: &Path, out_path: &Path) -> Result<()> {
    let sdl = std::fs::read_to_string(spec_path)
        .with_context(|| format!("failed to read App Registry schema {}", spec_path.display()))?;

    // Fail fast on a malformed vendor pull rather than committing something
    // `platform-app-client`'s build.rs can't parse either.
    async_graphql_parser::parse_schema(&sdl)
        .map_err(|e| anyhow::anyhow!("failed to parse App Registry schema: {e}"))?;
    if !sdl.contains("type Query") {
        bail!("App Registry schema has no `Query` type — source may have changed shape");
    }

    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    std::fs::write(out_path, &sdl).with_context(|| format!("write {}", out_path.display()))?;
    eprintln!("   wrote {}", out_path.display());
    Ok(())
}
