use std::{collections::HashMap, path::Path};

use anyhow::{Context, Result};
use async_graphql_parser::types::{
    FieldDefinition, InputValueDefinition, ServiceDocument, TypeKind, TypeSystemDefinition,
};
use async_graphql_value::ConstValue;
use serde::Serialize;

use crate::github::SpecSource;
use crate::openapi::{CatalogDomain, CatalogEndpoint, CatalogResponse};

// ---------------------------------------------------------------------------
// GraphQL output types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Clone)]
pub(crate) struct CatalogGraphqlArgument {
    pub(crate) name: String,
    #[serde(rename = "type")]
    pub(crate) arg_type: String,
    pub(crate) required: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) description: Option<String>,
    #[serde(rename = "defaultValue", skip_serializing_if = "Option::is_none")]
    pub(crate) default_value: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
pub(crate) struct CatalogGraphqlOperation {
    pub(crate) name: String,
    pub(crate) kind: String,
    #[serde(rename = "returnType")]
    pub(crate) return_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) description: Option<String>,
    pub(crate) deprecated: bool,
    #[serde(rename = "deprecationReason", skip_serializing_if = "Option::is_none")]
    pub(crate) deprecation_reason: Option<String>,
    pub(crate) args: Vec<CatalogGraphqlArgument>,
}

#[derive(Debug, Serialize, Clone)]
pub(crate) struct CatalogGraphqlField {
    pub(crate) name: String,
    #[serde(rename = "type")]
    pub(crate) field_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) description: Option<String>,
}

/// A named GraphQL object/input/enum type, captured so `api graphql get`/
/// `api graphql type get` can resolve an operation's return type (or any
/// nested field's type) into its real field list instead of a bare name.
#[derive(Debug, Serialize, Clone)]
pub(crate) struct CatalogGraphqlType {
    pub(crate) name: String,
    pub(crate) kind: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) fields: Vec<CatalogGraphqlField>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) values: Vec<String>,
}

#[derive(Debug, Serialize, Clone)]
pub(crate) struct CatalogGraphqlSchema {
    #[serde(rename = "schemaRef")]
    pub(crate) schema_ref: String,
    #[serde(rename = "operationCount")]
    pub(crate) operation_count: usize,
    pub(crate) operations: Vec<CatalogGraphqlOperation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) types: Vec<CatalogGraphqlType>,
    /// The raw GraphQL SDL source verbatim — see `api graphql sdl get`.
    #[serde(default)]
    pub(crate) sdl: String,
}

// ---------------------------------------------------------------------------
// GraphQL parsing
// ---------------------------------------------------------------------------

pub(crate) fn load_graphql_schema(
    path: &Path,
    schema_ref: &str,
    _common_types_dir: Option<&Path>,
) -> Result<CatalogGraphqlSchema> {
    let src = std::fs::read_to_string(path)
        .with_context(|| format!("cannot read GraphQL schema {}", path.display()))?;

    let (operations, types) = parse_graphql_document(&src).unwrap_or_else(|e| {
        eprintln!(
            "WARNING: failed to parse GraphQL schema {}: {e}",
            path.display()
        );
        (Vec::new(), Vec::new())
    });

    Ok(CatalogGraphqlSchema {
        schema_ref: schema_ref.to_owned(),
        operation_count: operations.len(),
        operations,
        types,
        sdl: src,
    })
}

/// Parses `source` with `async-graphql-parser` — chosen over the older
/// `graphql-parser` crate specifically because it natively understands
/// Apollo Federation's `extend schema @link(...)` directive, which several
/// upstream subgraph schemas use and `graphql-parser` cannot parse at all.
fn parse_graphql_document(
    source: &str,
) -> Result<(Vec<CatalogGraphqlOperation>, Vec<CatalogGraphqlType>)> {
    let doc: ServiceDocument = async_graphql_parser::parse_schema(source)
        .map_err(|e| anyhow::anyhow!("GraphQL parse error: {e}"))?;

    let mut operations: Vec<CatalogGraphqlOperation> = Vec::new();
    let mut types: Vec<CatalogGraphqlType> = Vec::new();

    for def in &doc.definitions {
        let TypeSystemDefinition::Type(pos_td) = def else {
            continue;
        };
        let td = &pos_td.node;

        match &td.kind {
            TypeKind::Object(obj) => match td.name.node.as_str() {
                "Query" => operations.extend(
                    obj.fields
                        .iter()
                        .map(|f| operation_from_field("query", &f.node)),
                ),
                "Mutation" => operations.extend(
                    obj.fields
                        .iter()
                        .map(|f| operation_from_field("mutation", &f.node)),
                ),
                name => types.push(CatalogGraphqlType {
                    name: name.to_owned(),
                    kind: "object".to_owned(),
                    fields: obj
                        .fields
                        .iter()
                        .map(|f| field_from_definition(&f.node))
                        .collect(),
                    values: Vec::new(),
                }),
            },
            TypeKind::InputObject(io) => types.push(CatalogGraphqlType {
                name: td.name.node.to_string(),
                kind: "input".to_owned(),
                fields: io
                    .fields
                    .iter()
                    .map(|f| input_field_from_definition(&f.node))
                    .collect(),
                values: Vec::new(),
            }),
            TypeKind::Enum(en) => types.push(CatalogGraphqlType {
                name: td.name.node.to_string(),
                kind: "enum".to_owned(),
                fields: Vec::new(),
                values: en
                    .values
                    .iter()
                    .map(|v| v.node.value.node.to_string())
                    .collect(),
            }),
            // Interfaces, unions, and scalars aren't modeled by `api
            // graphql type get` (there's no field/value list to drill
            // into for a bare scalar, and interface/union member
            // resolution isn't something a CLI call needs) — skipped.
            TypeKind::Interface(_) | TypeKind::Union(_) | TypeKind::Scalar => {}
        }
    }

    // Sort: queries before mutations, then alphabetical — matches the
    // original TypeScript CLI's ordering.
    operations.sort_by(|a, b| {
        if a.kind == b.kind {
            a.name.cmp(&b.name)
        } else if a.kind == "query" {
            std::cmp::Ordering::Less
        } else {
            std::cmp::Ordering::Greater
        }
    });
    types.sort_by(|a, b| a.name.cmp(&b.name));

    Ok((operations, types))
}

fn operation_from_field(kind: &str, field: &FieldDefinition) -> CatalogGraphqlOperation {
    let deprecated_directive = field
        .directives
        .iter()
        .find(|d| d.node.name.node.as_str() == "deprecated");
    let deprecation_reason = deprecated_directive.and_then(|d| {
        d.node
            .arguments
            .iter()
            .find(|(name, _)| name.node.as_str() == "reason")
            .and_then(|(_, v)| match &v.node {
                ConstValue::String(s) => Some(s.clone()),
                _ => None,
            })
    });

    let args: Vec<CatalogGraphqlArgument> = field
        .arguments
        .iter()
        .map(|a| {
            let a = &a.node;
            let required = !a.ty.node.nullable && a.default_value.is_none();
            CatalogGraphqlArgument {
                name: a.name.node.to_string(),
                arg_type: a.ty.node.to_string(),
                required,
                description: a.description.as_ref().map(|d| d.node.clone()),
                default_value: a.default_value.as_ref().map(|v| v.node.to_string()),
            }
        })
        .collect();

    CatalogGraphqlOperation {
        name: field.name.node.to_string(),
        kind: kind.to_owned(),
        return_type: field.ty.node.to_string(),
        description: field.description.as_ref().map(|d| d.node.clone()),
        deprecated: deprecated_directive.is_some(),
        deprecation_reason,
        args,
    }
}

fn field_from_definition(field: &FieldDefinition) -> CatalogGraphqlField {
    CatalogGraphqlField {
        name: field.name.node.to_string(),
        field_type: field.ty.node.to_string(),
        description: field.description.as_ref().map(|d| d.node.clone()),
    }
}

fn input_field_from_definition(field: &InputValueDefinition) -> CatalogGraphqlField {
    CatalogGraphqlField {
        name: field.name.node.to_string(),
        field_type: field.ty.node.to_string(),
        description: field.description.as_ref().map(|d| d.node.clone()),
    }
}

// ---------------------------------------------------------------------------
// GraphQL-only domain synthesis
// ---------------------------------------------------------------------------

pub(crate) fn synthesize_graphql_domain(
    source: &SpecSource,
    common_types_dir: Option<&Path>,
) -> CatalogDomain {
    let gql = load_graphql_schema(&source.spec_file, "./schema.graphql", common_types_dir)
        .unwrap_or_else(|e| {
            eprintln!(
                "WARNING: failed to load GraphQL schema for {}: {e}",
                source.domain
            );
            CatalogGraphqlSchema {
                schema_ref: "./schema.graphql".to_owned(),
                operation_count: 0,
                operations: Vec::new(),
                types: Vec::new(),
                sdl: String::new(),
            }
        });

    let op_count = gql.operation_count;
    CatalogDomain {
        name: source.domain.clone(),
        title: format!("{} GraphQL API", source.domain),
        description: format!("GraphQL API with {op_count} operations"),
        version: source
            .spec_version
            .strip_prefix('v')
            .unwrap_or(&source.spec_version)
            .to_owned(),
        base_url: String::new(),
        endpoints: vec![CatalogEndpoint {
            operation_id: "graphql".to_owned(),
            method: "POST".to_owned(),
            path: "/graphql".to_owned(),
            summary: "GraphQL API".to_owned(),
            description: Some(format!("GraphQL endpoint with {op_count} operations")),
            parameters: None,
            request_body: None,
            responses: {
                let mut r = HashMap::new();
                r.insert(
                    "200".to_owned(),
                    CatalogResponse {
                        description: "GraphQL response".to_owned(),
                        schema: None,
                    },
                );
                r
            },
            scopes: Vec::new(),
            graphql: Some(gql),
        }],
    }
}
