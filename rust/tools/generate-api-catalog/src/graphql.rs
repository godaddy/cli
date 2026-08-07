use std::{collections::HashMap, path::Path};

use anyhow::{Context, Result};
use serde::Serialize;

use crate::github::SpecSource;
use crate::openapi::{CatalogDomain, CatalogEndpoint, CatalogResponse};

// ---------------------------------------------------------------------------
// GraphQL output types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
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

#[derive(Debug, Serialize)]
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

#[derive(Debug, Serialize)]
pub(crate) struct CatalogGraphqlSchema {
    #[serde(rename = "schemaRef")]
    pub(crate) schema_ref: String,
    #[serde(rename = "operationCount")]
    pub(crate) operation_count: usize,
    pub(crate) operations: Vec<CatalogGraphqlOperation>,
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

    let operations = parse_graphql_operations(&src).unwrap_or_else(|e| {
        eprintln!(
            "WARNING: failed to parse GraphQL schema {}: {e}",
            path.display()
        );
        Vec::new()
    });

    Ok(CatalogGraphqlSchema {
        schema_ref: schema_ref.to_owned(),
        operation_count: operations.len(),
        operations,
    })
}

fn parse_graphql_operations(source: &str) -> Result<Vec<CatalogGraphqlOperation>> {
    use graphql_parser::schema::{Definition, TypeDefinition};

    let doc = graphql_parser::parse_schema::<String>(source)
        .map_err(|e| anyhow::anyhow!("GraphQL parse error: {e}"))?;

    let mut operations: Vec<CatalogGraphqlOperation> = Vec::new();

    for def in &doc.definitions {
        let type_def = match def {
            Definition::TypeDefinition(td) => td,
            _ => continue,
        };
        let obj = match type_def {
            TypeDefinition::Object(o) => o,
            _ => continue,
        };
        let kind = match obj.name.as_str() {
            "Query" => "query",
            "Mutation" => "mutation",
            _ => continue,
        };

        for field in &obj.fields {
            let deprecated = field.directives.iter().any(|d| d.name == "deprecated");
            let deprecation_reason = field
                .directives
                .iter()
                .find(|d| d.name == "deprecated")
                .and_then(|d| d.arguments.iter().find(|(k, _)| k == "reason"))
                .and_then(|(_, v)| {
                    if let graphql_parser::query::Value::String(s) = v {
                        Some(s.clone())
                    } else {
                        None
                    }
                });

            let args: Vec<CatalogGraphqlArgument> = field
                .arguments
                .iter()
                .map(|arg| {
                    let type_str = graphql_type_to_string(&arg.value_type);
                    let required =
                        matches!(arg.value_type, graphql_parser::schema::Type::NonNullType(_))
                            && arg.default_value.is_none();
                    let default_value = arg.default_value.as_ref().map(|v| format!("{v}"));
                    CatalogGraphqlArgument {
                        name: arg.name.clone(),
                        arg_type: type_str,
                        required,
                        description: arg.description.clone(),
                        default_value,
                    }
                })
                .collect();

            operations.push(CatalogGraphqlOperation {
                name: field.name.clone(),
                kind: kind.to_owned(),
                return_type: graphql_type_to_string(&field.field_type),
                description: field.description.clone(),
                deprecated,
                deprecation_reason,
                args,
            });
        }
    }

    // Sort: queries before mutations, then alphabetical
    operations.sort_by(|a, b| {
        if a.kind == b.kind {
            a.name.cmp(&b.name)
        } else if a.kind == "query" {
            std::cmp::Ordering::Less
        } else {
            std::cmp::Ordering::Greater
        }
    });

    Ok(operations)
}

fn graphql_type_to_string<'a, T: graphql_parser::query::Text<'a>>(
    t: &graphql_parser::schema::Type<'a, T>,
) -> String
where
    T::Value: std::fmt::Display,
{
    use graphql_parser::schema::Type;
    match t {
        Type::NamedType(name) => name.as_ref().to_string(),
        Type::NonNullType(inner) => format!("{}!", graphql_type_to_string(inner.as_ref())),
        Type::ListType(inner) => format!("[{}]", graphql_type_to_string(inner.as_ref())),
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
