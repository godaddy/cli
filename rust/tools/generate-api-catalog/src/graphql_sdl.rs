//! SDL helpers for the modules that vendor a GraphQL schema for a
//! `graphql_client`-generated client, so a transform written for one source is
//! available to the next instead of being copied.
//!
//! `catalog_products_spec` applies all of them. `app_registry_spec` uses only
//! [`ensure_query_root`]: it vendors its schema verbatim and has deprecated
//! nodes that `platform-app-client` still selects, so dropping them there is a
//! separate decision.
//!
//! Two parsers are involved on purpose. `async-graphql-parser` reads Apollo
//! Federation's `extend schema @link(...)` natively but cannot print SDL;
//! `graphql-parser` (the one `graphql_client` itself uses) prints SDL but
//! rejects `extend schema`. So [`strip_federation_schema_extension`] runs
//! before anything that needs `graphql-parser`.

use anyhow::{Result, bail};
use async_graphql_parser::types::{ServiceDocument, TypeKind, TypeSystemDefinition};
use graphql_parser::schema::{Definition, Directive, TypeDefinition, parse_schema};

/// Strips the leading `extend schema @link(...)` Apollo Federation directive
/// block a subgraph SDL declares before its first real definition.
/// `graphql_client`'s schema parser has no Federation support and rejects
/// `extend schema` outright, and the client queries never reference federation
/// directives, so dropping the block loses nothing. An SDL with no such block is
/// returned unchanged, so a schema without one still vendors correctly instead
/// of being mis-truncated.
pub(crate) fn strip_federation_schema_extension(sdl: &str) -> String {
    let trimmed = sdl.trim_start();
    if !trimmed.starts_with("extend schema") {
        return sdl.to_owned();
    }
    match trimmed.split_once("\n\n") {
        Some((_, rest)) => rest.to_owned(),
        None => sdl.to_owned(),
    }
}

fn is_deprecated(directives: &[Directive<'_, String>]) -> bool {
    directives
        .iter()
        .any(|directive| directive.name == "deprecated")
}

/// Parses `sdl`, removes every deprecated object field, interface field, input
/// field, enum value and field argument, and prints the result. Dropping them
/// means a client built from the schema cannot select or send a deprecated
/// member: code that still does stops compiling after a refresh, which forces a
/// move off the deprecated API. It also avoids codegen collisions where a
/// deprecated spelling sits next to its replacement (`HTMLDescription` beside
/// `htmlDescription`) and both normalize to one Rust identifier.
///
/// The printed form is normalized (whitespace and description quoting may differ
/// from the source), but every definition is kept and the output is stable under
/// another parse and print.
///
/// Fails if dropping leaves a type with no members at all, since the printed
/// schema would then be invalid. That means the upstream deprecated every member
/// of a type, which needs a human decision about the type, not a silent empty
/// block.
pub(crate) fn drop_deprecated_nodes(sdl: &str) -> Result<String> {
    let mut document = parse_schema::<String>(sdl)
        .map_err(|error| anyhow::anyhow!("the SDL does not parse: {error}"))?;

    for definition in &mut document.definitions {
        let Definition::TypeDefinition(type_definition) = definition else {
            continue;
        };
        match type_definition {
            TypeDefinition::Object(object) => {
                let before = object.fields.len();
                object
                    .fields
                    .retain(|field| !is_deprecated(&field.directives));
                for field in &mut object.fields {
                    field
                        .arguments
                        .retain(|argument| !is_deprecated(&argument.directives));
                }
                ensure_not_emptied(&object.name, before, object.fields.len())?;
            }
            TypeDefinition::Interface(interface) => {
                let before = interface.fields.len();
                interface
                    .fields
                    .retain(|field| !is_deprecated(&field.directives));
                for field in &mut interface.fields {
                    field
                        .arguments
                        .retain(|argument| !is_deprecated(&argument.directives));
                }
                ensure_not_emptied(&interface.name, before, interface.fields.len())?;
            }
            TypeDefinition::InputObject(input) => {
                let before = input.fields.len();
                input
                    .fields
                    .retain(|field| !is_deprecated(&field.directives));
                ensure_not_emptied(&input.name, before, input.fields.len())?;
            }
            TypeDefinition::Enum(enumeration) => {
                let before = enumeration.values.len();
                enumeration
                    .values
                    .retain(|value| !is_deprecated(&value.directives));
                ensure_not_emptied(&enumeration.name, before, enumeration.values.len())?;
            }
            TypeDefinition::Scalar(_) | TypeDefinition::Union(_) => {}
        }
    }

    Ok(document.to_string())
}

fn ensure_not_emptied(name: &str, before: usize, after: usize) -> Result<()> {
    if before > 0 && after == 0 {
        bail!(
            "dropping deprecated members leaves `{name}` with none; \
             every member of it is deprecated upstream"
        );
    }
    Ok(())
}

/// Fails fast on a malformed vendor pull rather than committing something the
/// client's build script can't parse either, and on a schema with no root
/// `Query` object: the catalog generator derives every query operation from an
/// object named exactly `Query`, so without it the catalog would silently list
/// none. `label` names the source in the error.
pub(crate) fn ensure_query_root(sdl: &str, label: &str) -> Result<()> {
    let document: ServiceDocument = async_graphql_parser::parse_schema(sdl)
        .map_err(|error| anyhow::anyhow!("failed to parse {label} schema: {error}"))?;
    let has_root_query = document.definitions.iter().any(|definition| {
        matches!(
            definition,
            TypeSystemDefinition::Type(type_definition)
                if type_definition.node.name.node == "Query"
                    && matches!(type_definition.node.kind, TypeKind::Object(_))
        )
    });
    if !has_root_query {
        bail!("{label} schema has no `Query` type — source may have changed shape");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn federation_extension_block_is_stripped() {
        let sdl = "extend schema\n  @link(url: \"https://specs.apollo.dev/federation/v2.6\", import: [])\n\ntype Query {\n  a: Int\n}\n";
        assert_eq!(
            strip_federation_schema_extension(sdl),
            "type Query {\n  a: Int\n}\n"
        );
    }

    #[test]
    fn an_sdl_without_the_block_is_returned_unchanged() {
        let sdl = "type Query {\n  a: Int\n}\n";
        assert_eq!(strip_federation_schema_extension(sdl), sdl);
    }

    #[test]
    fn leading_whitespace_before_the_block_is_tolerated() {
        let sdl = "\n\nextend schema @link(url: \"x\")\n\ntype Query {\n  a: Int\n}\n";
        assert_eq!(
            strip_federation_schema_extension(sdl),
            "type Query {\n  a: Int\n}\n"
        );
    }

    #[test]
    fn a_block_with_nothing_after_it_is_left_alone() {
        let sdl = "extend schema @link(url: \"x\")";
        assert_eq!(strip_federation_schema_extension(sdl), sdl);
    }

    /// Names of every member that survives, per type, so a test can assert on
    /// what is left without depending on the printer's formatting.
    fn members(sdl: &str) -> Vec<(String, Vec<String>)> {
        let document = parse_schema::<String>(sdl).expect("output parses");
        document
            .definitions
            .iter()
            .filter_map(|definition| match definition {
                Definition::TypeDefinition(TypeDefinition::Object(o)) => Some((
                    o.name.clone(),
                    o.fields.iter().map(|f| f.name.clone()).collect(),
                )),
                Definition::TypeDefinition(TypeDefinition::InputObject(i)) => Some((
                    i.name.clone(),
                    i.fields.iter().map(|f| f.name.clone()).collect(),
                )),
                Definition::TypeDefinition(TypeDefinition::Enum(e)) => Some((
                    e.name.clone(),
                    e.values.iter().map(|v| v.name.clone()).collect(),
                )),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn deprecated_object_and_input_fields_are_dropped_whatever_the_reason() {
        let sdl = r#"
type SKUGroup {
  "Old spelling."
  HTMLDescription: String @deprecated(reason: "Use htmlDescription instead.")
  htmlDescription: String
  shortLabel: String @deprecated(reason: "Use channel-specific overrides.")
  bare: Int @deprecated
  name: String!
}
input CreateInput {
  createdAt: String @deprecated(reason: "Sync only.")
  name: String
}
"#;
        let out = drop_deprecated_nodes(sdl).expect("transforms");
        assert_eq!(
            members(&out),
            vec![
                (
                    "SKUGroup".to_owned(),
                    vec!["htmlDescription".into(), "name".into()]
                ),
                ("CreateInput".to_owned(), vec!["name".into()]),
            ]
        );
        assert!(!out.contains("deprecated"), "{out}");
    }

    #[test]
    fn deprecated_operations_on_query_and_mutation_are_dropped() {
        let sdl = r#"
type Query {
  SKU(id: String!): String @deprecated(reason: "Use sku instead.")
  sku(id: String!): String
}
type Mutation {
  createSKUGroup(input: String!): String @deprecated(reason: "Use `createSkuGroup` instead.")
  createSkuGroup(input: String!): String
}
"#;
        let out = drop_deprecated_nodes(sdl).expect("transforms");
        assert_eq!(
            members(&out),
            vec![
                ("Query".to_owned(), vec!["sku".into()]),
                ("Mutation".to_owned(), vec!["createSkuGroup".into()]),
            ]
        );
    }

    #[test]
    fn deprecated_enum_values_and_arguments_are_dropped() {
        let sdl = r#"
enum Status {
  ACTIVE
  LEGACY @deprecated(reason: "Gone.")
}
type Query {
  items(first: Int, old: String @deprecated(reason: "Use first.")): [String]
}
"#;
        let out = drop_deprecated_nodes(sdl).expect("transforms");
        assert_eq!(
            members(&out)
                .into_iter()
                .find(|(name, _)| name == "Status")
                .map(|(_, values)| values),
            Some(vec!["ACTIVE".to_owned()])
        );
        let document = parse_schema::<String>(&out).expect("output parses");
        let arguments: Vec<_> = document
            .definitions
            .iter()
            .find_map(|definition| match definition {
                Definition::TypeDefinition(TypeDefinition::Object(o)) => Some(
                    o.fields[0]
                        .arguments
                        .iter()
                        .map(|a| a.name.clone())
                        .collect(),
                ),
                _ => None,
            })
            .unwrap_or_default();
        assert_eq!(arguments, ["first"]);
    }

    #[test]
    fn a_schema_with_nothing_deprecated_keeps_every_definition() {
        let sdl = "scalar DateTime\n\nunion U = A | B\n\ntype A {\n  a: Int\n}\n\ntype B {\n  b: Int\n}\n\ntype Query {\n  a: A\n}\n";
        let out = drop_deprecated_nodes(sdl).expect("transforms");
        assert_eq!(
            parse_schema::<String>(&out)
                .expect("output parses")
                .definitions
                .len(),
            5
        );
    }

    #[test]
    fn the_output_is_stable_under_another_pass() {
        let sdl = r#"
"""Docs that
span lines."""
type A {
  "One."
  a(x: Int = 1): [String!]! @deprecated(reason: "no")
  b: Int
}
"#;
        let once = drop_deprecated_nodes(sdl).expect("transforms");
        let twice = drop_deprecated_nodes(&once).expect("transforms again");
        assert_eq!(once, twice);
    }

    #[test]
    fn dropping_every_member_of_a_type_is_an_error_not_an_empty_block() {
        let sdl = "type Query {\n  a: Int\n}\ninput I {\n  x: Int @deprecated(reason: \"no\")\n}\n";
        let error = drop_deprecated_nodes(sdl).expect_err("an emptied input is rejected");
        assert!(error.to_string().contains("`I`"), "{error}");
    }

    #[test]
    fn a_type_that_was_already_empty_is_left_alone() {
        let sdl = "type Query {\n  a: Int\n}\ntype Empty\n";
        assert!(drop_deprecated_nodes(sdl).is_ok());
    }

    #[test]
    fn an_unparseable_schema_is_an_error() {
        let error = drop_deprecated_nodes("type {").expect_err("garbage does not parse");
        assert!(error.to_string().contains("does not parse"), "{error}");
    }

    #[test]
    fn accepts_a_schema_with_a_root_query_object() {
        ensure_query_root(
            "type Query { ping: String } type QueryThings { id: ID }",
            "Test",
        )
        .expect("schema with a root Query");
    }

    /// `async-graphql-parser` reads federation directives, so the check works on
    /// a schema before the block is stripped.
    #[test]
    fn a_root_query_is_found_in_a_schema_that_still_has_the_federation_block() {
        ensure_query_root(
            "extend schema @link(url: \"https://specs.apollo.dev/federation/v2.6\", import: [])\n\ntype Query { a: Int }",
            "Test",
        )
        .expect("federation block does not hide the root Query");
    }

    #[test]
    fn rejects_connection_types_without_a_root_query() {
        // `type QueryApplicationsConnection` contains the text `type Query`,
        // which a substring check would wrongly accept.
        let error = ensure_query_root("type QueryApplicationsConnection { total: Int }", "Test")
            .expect_err("no root Query")
            .to_string();
        assert!(error.contains("Test schema has no `Query` type"), "{error}");
    }

    #[test]
    fn rejects_a_query_name_that_is_not_an_object_type() {
        ensure_query_root("input Query { id: ID }", "Test").expect_err("Query must be an object");
    }

    #[test]
    fn rejects_unparseable_schema_and_names_the_source() {
        let error = ensure_query_root("type Query {", "Test")
            .expect_err("malformed")
            .to_string();
        assert!(error.contains("failed to parse Test schema"), "{error}");
    }
}
