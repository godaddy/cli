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
use async_graphql_parser::{
    Pos,
    types::{ServiceDocument, TypeKind, TypeSystemDefinition},
};
use graphql_parser::schema::{Definition, Directive, TypeDefinition, TypeExtension, parse_schema};

/// Removes every `extend schema ...` definition (the Apollo Federation
/// `extend schema @link(...)` block a subgraph SDL declares) and returns the
/// rest of the text untouched. `graphql_client`'s schema parser has no
/// Federation support and rejects `extend schema` outright, and the client
/// queries never reference federation directives, so dropping them loses
/// nothing. An SDL with none is returned unchanged.
///
/// The extension's extent comes from the parser, not from the text: it runs
/// from where the extension starts to where the next definition starts, so a
/// definition that follows it directly (one newline, a description, a comment
/// or even the same line) is kept. Fails if the SDL does not parse, which would
/// make that extent unknowable.
pub(crate) fn strip_federation_schema_extension(sdl: &str) -> Result<String> {
    let document: ServiceDocument = async_graphql_parser::parse_schema(sdl)
        .map_err(|error| anyhow::anyhow!("the SDL does not parse: {error}"))?;

    // (byte offset of the definition's start, whether it is `extend schema`),
    // in source order.
    let starts: Vec<(usize, bool)> = document
        .definitions
        .iter()
        .map(|definition| match definition {
            TypeSystemDefinition::Schema(schema) => {
                (byte_offset(sdl, schema.pos), schema.node.extend)
            }
            TypeSystemDefinition::Type(type_definition) => {
                (byte_offset(sdl, type_definition.pos), false)
            }
            TypeSystemDefinition::Directive(directive) => (byte_offset(sdl, directive.pos), false),
        })
        .collect();
    if !starts.iter().any(|(_, is_extension)| *is_extension) {
        return Ok(sdl.to_owned());
    }

    let mut kept = String::with_capacity(sdl.len());
    let mut cursor = 0;
    for (index, (start, is_extension)) in starts.iter().enumerate() {
        if !is_extension {
            continue;
        }
        let end = starts
            .get(index + 1)
            .map_or(sdl.len(), |(next_start, _)| *next_start);
        kept.push_str(&sdl[cursor..*start]);
        cursor = end;
    }
    kept.push_str(&sdl[cursor..]);
    Ok(kept)
}

/// Converts the parser's one-based line and (character) column into a byte
/// offset into `sdl`, clamped to the end of the text.
fn byte_offset(sdl: &str, position: Pos) -> usize {
    let line_start: usize = sdl
        .split_inclusive('\n')
        .take(position.line.saturating_sub(1))
        .map(str::len)
        .sum();
    let line = &sdl[line_start..];
    let within_line = line
        .char_indices()
        .nth(position.column.saturating_sub(1))
        .map_or(line.len(), |(offset, _)| offset);
    line_start + within_line
}

fn is_deprecated(directives: &[Directive<'_, String>]) -> bool {
    directives
        .iter()
        .any(|directive| directive.name == "deprecated")
}

/// Parses `sdl`, removes every deprecated object field, interface field, input
/// field, enum value and field or directive argument, in type definitions and in
/// type extensions alike, and prints the result. Dropping them means a client
/// built from the schema cannot select or send a deprecated member: code that
/// still does stops compiling after a refresh, which forces a move off the
/// deprecated API. It also avoids codegen collisions where a deprecated spelling
/// sits next to its replacement (`HTMLDescription` beside `htmlDescription`) and
/// both normalize to one Rust identifier.
///
/// The printed form is normalized (whitespace and description quoting may differ
/// from the source), but every definition is kept and the output is stable under
/// another parse and print. A type extension left with nothing in it (no members,
/// directives or interfaces) is removed, since it would add nothing and print as
/// an invalid empty block.
///
/// Fails if dropping leaves a type definition with no members at all, since the
/// printed schema would then be invalid. That means the upstream deprecated every
/// member of a type, which needs a human decision about the type, not a silent
/// empty block.
pub(crate) fn drop_deprecated_nodes(sdl: &str) -> Result<String> {
    let mut document = parse_schema::<String>(sdl)
        .map_err(|error| anyhow::anyhow!("the SDL does not parse: {error}"))?;

    for definition in &mut document.definitions {
        match definition {
            Definition::SchemaDefinition(_) => {}
            Definition::TypeDefinition(type_definition) => {
                drop_in_type_definition(type_definition)?;
            }
            Definition::TypeExtension(type_extension) => drop_in_type_extension(type_extension),
            Definition::DirectiveDefinition(directive) => {
                directive
                    .arguments
                    .retain(|argument| !is_deprecated(&argument.directives));
            }
        }
    }
    document.definitions.retain(|definition| {
        !matches!(definition, Definition::TypeExtension(extension) if extension_is_empty(extension))
    });

    Ok(document.to_string())
}

fn drop_in_type_definition(type_definition: &mut TypeDefinition<'_, String>) -> Result<()> {
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
            ensure_not_emptied(&object.name, before, object.fields.len())
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
            ensure_not_emptied(&interface.name, before, interface.fields.len())
        }
        TypeDefinition::InputObject(input) => {
            let before = input.fields.len();
            input
                .fields
                .retain(|field| !is_deprecated(&field.directives));
            ensure_not_emptied(&input.name, before, input.fields.len())
        }
        TypeDefinition::Enum(enumeration) => {
            let before = enumeration.values.len();
            enumeration
                .values
                .retain(|value| !is_deprecated(&value.directives));
            ensure_not_emptied(&enumeration.name, before, enumeration.values.len())
        }
        TypeDefinition::Scalar(_) | TypeDefinition::Union(_) => Ok(()),
    }
}

/// The extension counterpart of [`drop_in_type_definition`]. An extension may
/// legitimately end up empty (its members were all deprecated), which the caller
/// handles by removing it, so this does not fail.
fn drop_in_type_extension(type_extension: &mut TypeExtension<'_, String>) {
    match type_extension {
        TypeExtension::Object(object) => {
            object
                .fields
                .retain(|field| !is_deprecated(&field.directives));
            for field in &mut object.fields {
                field
                    .arguments
                    .retain(|argument| !is_deprecated(&argument.directives));
            }
        }
        TypeExtension::Interface(interface) => {
            interface
                .fields
                .retain(|field| !is_deprecated(&field.directives));
            for field in &mut interface.fields {
                field
                    .arguments
                    .retain(|argument| !is_deprecated(&argument.directives));
            }
        }
        TypeExtension::InputObject(input) => {
            input
                .fields
                .retain(|field| !is_deprecated(&field.directives));
        }
        TypeExtension::Enum(enumeration) => {
            enumeration
                .values
                .retain(|value| !is_deprecated(&value.directives));
        }
        TypeExtension::Scalar(_) | TypeExtension::Union(_) => {}
    }
}

/// True if an extension adds nothing: no members, directives or interfaces.
fn extension_is_empty(type_extension: &TypeExtension<'_, String>) -> bool {
    match type_extension {
        TypeExtension::Object(object) => {
            object.fields.is_empty()
                && object.directives.is_empty()
                && object.implements_interfaces.is_empty()
        }
        TypeExtension::Interface(interface) => {
            interface.fields.is_empty()
                && interface.directives.is_empty()
                && interface.implements_interfaces.is_empty()
        }
        TypeExtension::InputObject(input) => input.fields.is_empty() && input.directives.is_empty(),
        TypeExtension::Enum(enumeration) => {
            enumeration.values.is_empty() && enumeration.directives.is_empty()
        }
        TypeExtension::Scalar(scalar) => scalar.directives.is_empty(),
        TypeExtension::Union(union) => union.types.is_empty() && union.directives.is_empty(),
    }
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

    fn strip(sdl: &str) -> String {
        strip_federation_schema_extension(sdl).expect("strips")
    }

    #[test]
    fn federation_extension_block_is_stripped() {
        let sdl = "extend schema\n  @link(url: \"https://specs.apollo.dev/federation/v2.6\", import: [])\n\ntype Query {\n  a: Int\n}\n";
        assert_eq!(strip(sdl), "type Query {\n  a: Int\n}\n");
    }

    /// The regression the review found: with a single newline after the
    /// extension there is no blank line to split on, and the definition right
    /// after it must survive.
    #[test]
    fn a_definition_right_after_the_extension_survives_a_single_newline() {
        let sdl = "extend schema @link(url: \"x\")\ntype AddListTreeNodeInput {\n  a: Int\n}\n\ntype Query {\n  a: Int\n}\n";
        let out = strip(sdl);
        assert!(
            out.starts_with("type AddListTreeNodeInput {"),
            "the next definition was lost: {out:?}"
        );
        assert!(out.contains("type Query {"), "{out:?}");
    }

    #[test]
    fn a_description_on_the_next_definition_is_kept() {
        let sdl = "extend schema @link(url: \"x\")\n\"\"\"Doc.\"\"\"\ntype A {\n  a: Int\n}\n";
        assert_eq!(strip(sdl), "\"\"\"Doc.\"\"\"\ntype A {\n  a: Int\n}\n");
    }

    #[test]
    fn a_multi_line_extension_with_several_directives_is_removed_whole() {
        let sdl = "extend schema\n  @link(url: \"x\", import: [])\n  @other\n\n# about A\ntype A {\n  a: Int\n}\n";
        let out = strip(sdl);
        assert!(
            !out.contains("extend") && !out.contains("@other"),
            "{out:?}"
        );
        assert!(out.contains("type A {"), "{out:?}");
    }

    /// A definition on the same line as the end of the extension, after a
    /// multi-byte character, so both the line and the column unit are checked.
    #[test]
    fn a_definition_on_the_same_line_after_a_multibyte_character_survives() {
        let sdl = "extend schema @link(url: \"é\") type A { a: Int }\n";
        assert_eq!(strip(sdl), "type A { a: Int }\n");
    }

    #[test]
    fn extensions_anywhere_in_the_document_are_removed_and_the_rest_kept() {
        let sdl = "type A {\n  a: Int\n}\n\nextend schema @link(url: \"x\")\ntype B {\n  b: Int\n}\nextend schema @link(url: \"y\")\n\ntype Query {\n  a: Int\n}\n";
        let out = strip(sdl);
        assert!(!out.contains("extend"), "{out:?}");
        for name in ["type A {", "type B {", "type Query {"] {
            assert!(out.contains(name), "{name} was lost: {out:?}");
        }
    }

    #[test]
    fn an_sdl_without_the_block_is_returned_unchanged() {
        let sdl = "type Query {\n  a: Int\n}\n";
        assert_eq!(strip(sdl), sdl);
    }

    #[test]
    fn a_schema_definition_that_is_not_an_extension_is_left_alone() {
        let sdl = "schema {\n  query: Query\n}\n\ntype Query {\n  a: Int\n}\n";
        assert_eq!(strip(sdl), sdl);
    }

    #[test]
    fn leading_whitespace_before_the_block_is_tolerated() {
        let sdl = "\n\nextend schema @link(url: \"x\")\n\ntype Query {\n  a: Int\n}\n";
        assert_eq!(strip(sdl).trim_start(), "type Query {\n  a: Int\n}\n");
    }

    #[test]
    fn a_block_with_nothing_after_it_leaves_nothing() {
        assert!(strip("extend schema @link(url: \"x\")").trim().is_empty());
    }

    #[test]
    fn stripping_an_unparseable_sdl_is_an_error() {
        let error = strip_federation_schema_extension("type {").expect_err("does not parse");
        assert!(error.to_string().contains("does not parse"), "{error}");
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
    fn deprecated_members_in_type_extensions_are_dropped_too() {
        let sdl = r#"
type Query { ping: String }
extend type Query { legacy: String @deprecated(reason: "x") keep: String }
input I { a: Int }
extend input I { legacyInput: Int @deprecated(reason: "x") keepInput: Int }
enum E { A }
extend enum E { LEGACY @deprecated(reason: "x") B }
interface N { id: ID }
extend interface N { old: Int @deprecated(reason: "x") fresh: Int }
"#;
        let out = drop_deprecated_nodes(sdl).expect("transforms");
        for dropped in ["legacy", "legacyInput", "LEGACY", "old"] {
            assert!(!out.contains(dropped), "{dropped} survived: {out}");
        }
        for kept in ["keep", "keepInput", "fresh", "extend type Query", "B"] {
            assert!(out.contains(kept), "{kept} was lost: {out}");
        }
        assert!(!out.contains("deprecated"), "{out}");
    }

    #[test]
    fn an_extension_whose_members_were_all_deprecated_is_removed() {
        let sdl =
            "type Query { a: Int }\nextend type Query { old: Int @deprecated(reason: \"x\") }\n";
        let out = drop_deprecated_nodes(sdl).expect("an emptied extension is not an error");
        assert!(!out.contains("extend"), "{out}");
        parse_schema::<String>(&out).expect("the output still parses");
    }

    #[test]
    fn an_emptied_extension_that_still_has_directives_is_kept() {
        let sdl = "type Query { a: Int }\nextend type Query @key(fields: \"a\") { old: Int @deprecated(reason: \"x\") }\n";
        let out = drop_deprecated_nodes(sdl).expect("transforms");
        assert!(out.contains("extend type Query"), "{out}");
        assert!(out.contains("@key"), "{out}");
        assert!(!out.contains("old"), "{out}");
    }

    #[test]
    fn deprecated_directive_arguments_are_dropped() {
        let sdl = "type Query { a: Int }\ndirective @filter(old: String @deprecated(reason: \"x\"), keep: String) on FIELD\n";
        let out = drop_deprecated_nodes(sdl).expect("transforms");
        assert!(!out.contains("old"), "{out}");
        assert!(out.contains("keep"), "{out}");
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
