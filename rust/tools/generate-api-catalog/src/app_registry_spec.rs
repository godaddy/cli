//! Refreshes the vendored App Registry GraphQL schema used by
//! `platform-app-client`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use async_graphql_parser::types::{ServiceDocument, TypeKind, TypeSystemDefinition};

pub(crate) fn app_registry_client_schema_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../platform-app-client/graphql/schema.graphql")
}

/// Fails fast on a malformed vendor pull rather than committing something
/// `platform-app-client`'s build.rs can't parse either, and on one with no
/// root `Query` object — the catalog generator derives every query operation
/// from an object named exactly `Query`, so without it the catalog would
/// silently list none.
fn validate_schema(sdl: &str) -> Result<()> {
    let doc: ServiceDocument = async_graphql_parser::parse_schema(sdl)
        .map_err(|e| anyhow::anyhow!("failed to parse App Registry schema: {e}"))?;
    let has_root_query = doc.definitions.iter().any(|definition| {
        matches!(
            definition,
            TypeSystemDefinition::Type(td)
                if td.node.name.node == "Query" && matches!(td.node.kind, TypeKind::Object(_))
        )
    });
    if !has_root_query {
        bail!("App Registry schema has no `Query` type — source may have changed shape");
    }
    Ok(())
}

pub(crate) fn refresh(spec_path: &Path, out_path: &Path) -> Result<()> {
    let sdl = std::fs::read_to_string(spec_path)
        .with_context(|| format!("failed to read App Registry schema {}", spec_path.display()))?;

    validate_schema(&sdl)?;

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
    fn accepts_a_schema_with_a_root_query_object() {
        validate_schema("type Query { ping: String } type QueryThings { id: ID }")
            .expect("schema with a root Query");
    }

    #[test]
    fn rejects_connection_types_without_a_root_query() {
        // `type QueryApplicationsConnection` contains the text `type Query`,
        // which a substring check would wrongly accept.
        let error = validate_schema("type QueryApplicationsConnection { total: Int }")
            .expect_err("no root Query")
            .to_string();
        assert!(error.contains("no `Query` type"), "{error}");
    }

    #[test]
    fn rejects_a_query_name_that_is_not_an_object_type() {
        validate_schema("input Query { id: ID }").expect_err("Query must be an object");
    }

    #[test]
    fn rejects_unparseable_schema() {
        let error = validate_schema("type Query {")
            .expect_err("malformed")
            .to_string();
        assert!(error.contains("failed to parse"), "{error}");
    }

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
