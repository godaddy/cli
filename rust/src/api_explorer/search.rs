//! `api search`.

use cli_engine::{
    CommandResult, CommandSpec, NextActionParam, PaginationConfig, RuntimeCommandSpec, Tier,
};
use serde_json::{Value, json};

use crate::next_action::next_action;
use crate::output_schema::output_schema;

use super::catalog::{catalog, graphql_sub_endpoint_rows, search_endpoints};

output_schema!(ApiEndpoint {
    "domain": "string";
    "operationId": "string";
    "method": "string";
    "path": "string";
    "summary": "string", optional;
    "scopes": "[]string";
    "graphqlOperations": "number", optional;
    // See `ApiDomainEndpoint`'s `kind` field in `operation.rs` — present
    // only on a synthetic row for one addressable GraphQL query/mutation.
    "kind": "string", optional;
});

#[derive(Debug, Clone, clap::Args)]
struct SearchArgs {
    /// Search term (matches path, operationId, summary, description).
    #[arg(value_name = "QUERY")]
    query: String,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed::<SearchArgs, _, _, _>(
        CommandSpec::from_args::<SearchArgs>("search", "Search API endpoints by keyword")
            .with_long(
                "Full-text searches across all API domains, matching against \
                operation IDs, paths, and descriptions. Use \
                `api operation get <operationId>` to inspect a result in full \
                detail.",
            )
            .with_system("api")
            .with_tier(Tier::Read)
            .no_auth(true)
            .with_default_fields("operationId,domain,method,path,summary")
            .with_output_schema::<ApiEndpoint>()
            .with_pagination(PaginationConfig {
                max_limit: 100,
                ..Default::default()
            }),
        |_cred, args: SearchArgs| async move {
            let hits = search_endpoints(catalog(), &args.query);
            if hits.is_empty() {
                return Ok(CommandResult::new(json!([])));
            }
            let mut results: Vec<Value> = hits
                .iter()
                .map(|(domain, ep)| {
                    json!({
                        "domain": domain.name,
                        "operationId": ep.operation_id,
                        "method": ep.method,
                        "path": ep.path,
                        "summary": ep.summary,
                        "scopes": ep.scopes,
                        "graphqlOperations": ep.graphql.as_ref().map(|g| g.operation_count),
                    })
                })
                .collect();
            // A matched GraphQL wrapper's own query/mutation fields are
            // just as addressable as any REST operation — surface whichever
            // of them also match the search term as their own rows.
            let query_lower = args.query.to_lowercase();
            for (domain, ep) in &hits {
                for row in graphql_sub_endpoint_rows(Some(&domain.name), ep) {
                    let matches = row
                        .get("operationId")
                        .and_then(Value::as_str)
                        .is_some_and(|s| s.to_lowercase().contains(&query_lower))
                        || row
                            .get("summary")
                            .and_then(Value::as_str)
                            .is_some_and(|s| s.to_lowercase().contains(&query_lower));
                    if matches {
                        results.push(row);
                    }
                }
            }

            let mut next_actions = vec![
                next_action(
                    "api operation get <operation>",
                    "Get full details for a result",
                )
                .with_param("operation", NextActionParam::required()),
            ];
            let graphql_wrappers: Vec<&str> = {
                let mut wrappers: Vec<&str> = results
                    .iter()
                    .filter(|r| r.get("kind").is_some())
                    .filter_map(|r| r.get("operationId").and_then(Value::as_str))
                    .filter_map(|id| id.split_once("::").map(|(wrapper, _)| wrapper))
                    .collect();
                wrappers.sort_unstable();
                wrappers.dedup();
                wrappers
            };
            if !graphql_wrappers.is_empty() {
                next_actions.push(
                    next_action(
                        "api graphql get <operation>",
                        "See a GraphQL result's own shape (arguments and real return type)",
                    )
                    .with_param("operation", NextActionParam::required()),
                );
            }

            Ok(CommandResult::new(json!(results)).with_next_actions(next_actions))
        },
    )
}

#[cfg(test)]
mod tests {
    use cli_engine::PaginationConfig;

    use super::command;

    #[test]
    fn command_opts_into_pagination_with_no_default_and_a_max_limit() {
        assert_eq!(
            command().spec.pagination,
            Some(PaginationConfig {
                max_limit: 100,
                ..Default::default()
            })
        );
    }
}
