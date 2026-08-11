//! `api graphql get`.

use cli_engine::{
    CommandResult, CommandSpec, NextActionParam, RuntimeCommandSpec, TableColumn, Tier,
};
use serde_json::{Value, json};

use crate::next_action::next_action;
use crate::output_schema::output_schema;

use super::super::catalog::{
    catalog, graphql_operation_id, graphql_operation_not_found_error, graphql_resolve_type,
    resolve_graphql_operation,
};
use super::super::schema_tree::describe_schema;

// A GraphQL operation
output_schema!(ApiGraphqlOperation {
    "domain": "string";
    "operationId": "string";
    "kind": "string";
    "name": "string";
    "summary": "string", optional;
    "description": "string", optional;
    "deprecated": "bool";
    "deprecationReason": "string", optional;
    "returnType": "string";
    "returnFields": "[]object", optional;
    "callRequirements": "[]object";
    "arguments": "[]object";
    "scopes": "[]string";
});

#[derive(Debug, Clone, clap::Args)]
struct GraphqlGetArgs {
    /// GraphQL operation id (see `api operation list --domain <domain>` or `api search`).
    #[arg(value_name = "ID")]
    id: String,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed::<GraphqlGetArgs, _, _, _>(
        CommandSpec::from_args::<GraphqlGetArgs>("get", "Show a GraphQL operation's own shape")
            .with_long(
                "Shows a GraphQL operation's details. \"Call Requirements\" \
                lists what the wrapper endpoint that proxies this operation \
                needs to route and authenticate the request. \"Arguments\" lists \
                 the operation's GraphQL inputs. Use `api graphql call <id>` to \
                 execute it.",
            )
            .with_system("api")
            .with_tier(Tier::Read)
            .no_auth(true)
            .with_output_schema::<ApiGraphqlOperation>()
            .with_view(vec![
                TableColumn::new("domain", "Domain"),
                TableColumn::new("operationId", "Operation ID"),
                TableColumn::new("kind", "Kind"),
                TableColumn::new("summary", "Summary"),
                TableColumn::new("returnType", "Return Type"),
                TableColumn::new("returnFields", "Return Fields").nested(vec![
                    TableColumn::new("name", "Name"),
                    TableColumn::new("type", "Type"),
                    TableColumn::new("description", "Description"),
                ]),
                TableColumn::new("callRequirements", "Call Requirements").nested(vec![
                    TableColumn::new("name", "Name"),
                    TableColumn::new("in", "In"),
                    TableColumn::new("required", "Required"),
                    TableColumn::new("type", "Type"),
                    TableColumn::new("description", "Description"),
                ]),
                TableColumn::new("arguments", "Arguments").nested(vec![
                    TableColumn::new("name", "Name"),
                    TableColumn::new("required", "Required"),
                    TableColumn::new("type", "Type"),
                    TableColumn::new("description", "Description"),
                ]),
            ]),
        |_cred, args: GraphqlGetArgs| async move {
            let id = args.id.as_str();
            let g = resolve_graphql_operation(catalog(), id)
                .ok_or_else(|| graphql_operation_not_found_error(id))?;

            // Most return types resolve to an object/input with real fields;
            // a bare enum return (no known operation does this today, but
            // the schema allows it) has no `fields` at all — its "shape" is
            // its set of allowed values instead.
            let return_fields = graphql_resolve_type(&g, &g.op.return_type).map(|t| {
                if t.kind == "enum" {
                    t.values
                        .iter()
                        .map(|v| json!({"name": v, "type": "enum value"}))
                        .collect::<Vec<_>>()
                } else {
                    t.fields
                        .iter()
                        .map(|f| {
                            json!({"name": f.name, "type": f.field_type, "description": f.description})
                        })
                        .collect::<Vec<_>>()
                }
            });

            let call_requirements: Vec<Value> = g
                .parent
                .parameters
                .iter()
                .filter_map(|p| {
                    let obj = p.as_object()?;
                    let name = obj.get("name")?.as_str()?.to_owned();
                    let location = obj
                        .get("in")
                        .and_then(Value::as_str)
                        .unwrap_or("query")
                        .to_owned();
                    let required = obj
                        .get("required")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    let (scalar_type, _) = describe_schema(obj.get("schema"), &g.domain.defs);
                    let description = obj
                        .get("description")
                        .and_then(Value::as_str)
                        .map(str::to_owned);
                    Some(json!({
                        "name": name,
                        "in": location,
                        "required": required,
                        "type": scalar_type,
                        "description": description,
                    }))
                })
                .collect();

            let arguments: Vec<Value> =
                g.op.args
                    .iter()
                    .map(|a| {
                        json!({
                            "name": a.name,
                            "type": a.arg_type,
                            "required": a.required,
                            "description": a.description,
                            "defaultValue": a.default_value,
                        })
                    })
                    .collect();

            let synthetic_id = graphql_operation_id(&g.parent.operation_id, &g.op.kind, &g.op.name);
            let has_return_fields = return_fields.is_some();

            let mut next_actions = vec![next_action(
                format!("api graphql call {synthetic_id} --arg name=value"),
                "Make an authenticated call to this operation",
            )];
            // Only worth suggesting when the return type actually resolved
            // to something with fields to drill into — a caller with a
            // plain-scalar return type has nowhere further to go.
            if has_return_fields {
                next_actions.push(
                    next_action(
                        "api graphql type get <TypeName>",
                        "See a nested type's own fields — pass any Return Fields Type value \
                         that isn't a plain scalar (e.g. ClassificationListsConnection)",
                    )
                    .with_param("TypeName", NextActionParam::required()),
                );
            }

            Ok(CommandResult::new(json!({
                "domain": g.domain.name,
                "operationId": synthetic_id,
                "kind": g.op.kind,
                "name": g.op.name,
                "summary": g
                    .op
                    .description
                    .clone()
                    .unwrap_or_else(|| format!("{} {}", g.op.kind, g.op.name)),
                "description": g.op.description,
                "deprecated": g.op.deprecated,
                "deprecationReason": g.op.deprecation_reason,
                "returnType": g.op.return_type,
                "returnFields": return_fields,
                "callRequirements": call_requirements,
                "arguments": arguments,
                "scopes": g.parent.scopes,
            }))
            .with_next_actions(next_actions))
        },
    )
}

#[cfg(test)]
mod tests {
    use cli_engine::{Cli, CliConfig};
    use serde_json::Value;

    fn graphql_cli() -> Cli {
        Cli::new(
            CliConfig::new("gddy", "GoDaddy developer CLI", "gddy")
                .with_module(crate::api_explorer::module()),
        )
    }

    fn find_a_graphql_operation_id() -> String {
        // Any real query/mutation on the taxes domain — walk the catalog
        // directly rather than hardcoding a name that upstream spec drift
        // could rename out from under this test.
        let (_, ep) = super::super::super::catalog::find_endpoint(
            super::super::super::catalog::catalog(),
            "postTaxGraphql",
        )
        .expect("postTaxGraphql exists in the embedded taxes catalog");
        let schema = ep
            .graphql
            .as_ref()
            .expect("postTaxGraphql has a graphql schema");
        let op = schema
            .operations
            .first()
            .expect("taxes graphql schema has at least one operation");
        super::super::super::catalog::graphql_operation_id(&ep.operation_id, &op.kind, &op.name)
    }

    #[tokio::test]
    async fn graphql_get_returns_kind_return_shape_and_split_requirements_vs_arguments() {
        let id = find_a_graphql_operation_id();
        let output = graphql_cli()
            .run(["gddy", "api", "graphql", "get", &id, "--output", "json"])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let rendered: Value = serde_json::from_str(&output.rendered).expect("valid json");
        let data = &rendered["data"];
        assert!(data["kind"].as_str().is_some());
        assert!(data.get("method").is_none(), "no REST fields: {data}");
        assert!(data.get("path").is_none(), "no REST fields: {data}");
        assert!(data.get("responses").is_none(), "no REST fields: {data}");
    }

    #[tokio::test]
    async fn graphql_get_unknown_id_is_not_found() {
        let output = graphql_cli()
            .run([
                "gddy",
                "api",
                "graphql",
                "get",
                "postTaxGraphql::query::notARealField",
                "--output",
                "json",
            ])
            .await;
        assert_ne!(output.exit_code, 0, "{}", output.rendered);
    }
}
