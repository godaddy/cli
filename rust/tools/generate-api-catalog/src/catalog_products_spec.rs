//! Vendors the Catalog Products GraphQL schema to
//! `schemas/graphql/catalog-products.graphql`, where `graphql_client` can
//! check queries against it at compile time.
//!
//! `catalog-products` is not a bare GraphQL source: it has one REST wrapper
//! operation (`POST /stores/{storeId}/catalog-subgraph`) whose
//! `x-godaddy-graphql-schema` extension carries the real SDL, which the catalog
//! processing already resolves into the endpoint's `graphql.sdl`. That text is
//! what this module writes, after the shared transforms in `graphql_sdl`:
//! the Apollo Federation block is stripped and every `@deprecated` node is
//! dropped (see that module for why).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::{graphql_sdl, openapi::CatalogDomain};

/// The catalog domain whose SDL is vendored.
pub(crate) const DOMAIN: &str = "catalog-products";

pub(crate) fn catalog_products_graphql_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../schemas/graphql/catalog-products.graphql")
}

/// Writes the domain's SDL to `out_path`, or warns and leaves `out_path`
/// untouched if the catalog entry carries none. A missing SDL is not worth
/// failing the whole catalog run over, the same trade the domains `v1` pull
/// makes, but it must not pass silently. A schema that cannot be transformed
/// is an error, so a broken pull never overwrites the last usable file.
pub(crate) fn vendor_sdl(catalog: &CatalogDomain, out_path: &Path) -> Result<()> {
    let Some(sdl) = catalog
        .endpoints
        .iter()
        .find_map(|endpoint| endpoint.graphql.as_ref())
        .map(|graphql| graphql.sdl.as_str())
        .filter(|sdl| !sdl.is_empty())
    else {
        eprintln!(
            "WARNING: {DOMAIN} has no GraphQL SDL to vendor — leaving {} untouched",
            out_path.display()
        );
        return Ok(());
    };

    let sdl = graphql_sdl::strip_federation_schema_extension(sdl)?;
    let sdl = graphql_sdl::drop_deprecated_nodes(&sdl)?;
    graphql_sdl::ensure_query_root(&sdl, DOMAIN)?;
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    std::fs::write(out_path, sdl)
        .with_context(|| format!("failed to write {}", out_path.display()))?;
    eprintln!("   wrote {}", out_path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use graphql_parser::schema::{Definition, TypeDefinition, parse_schema};

    /// Drift guard on the committed file, runnable offline: it must be valid SDL
    /// with no federation block and no deprecated node, which is what a client
    /// built from it relies on. Catches a hand edit or a stale vendored copy.
    #[test]
    fn the_vendored_schema_has_no_federation_block_and_no_deprecated_nodes() {
        let sdl = std::fs::read_to_string(catalog_products_graphql_path())
            .expect("the vendored catalog-products schema exists");
        assert!(!sdl.trim_start().starts_with("extend schema"));
        let document = parse_schema::<String>(&sdl).expect("the vendored schema parses");
        assert!(
            !document.to_string().contains("@deprecated"),
            "a deprecated node survived vendoring"
        );
        assert!(
            document.definitions.iter().any(|definition| matches!(
                definition,
                Definition::TypeDefinition(TypeDefinition::Object(object)) if object.name == "Query"
            )),
            "the vendored schema has no Query type"
        );
    }
}
