//! `api search`.

use cli_engine::{CommandResult, CommandSpec, NextActionParam, RuntimeCommandSpec, Tier};
use serde_json::{Value, json};

use crate::next_action::next_action;
use crate::output_schema::output_schema;

use super::catalog::{catalog, search_endpoints};

output_schema!(ApiEndpoint {
    "domain": "string";
    "operationId": "string";
    "method": "string";
    "path": "string";
    "summary": "string", optional;
    "scopes": "[]string";
    "graphqlOperations": "number", optional;
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
                "Full-text searches across all API domains, matching against operation IDs, \
                 paths, summaries, and descriptions. Returns matching endpoints with domain, \
                 method, path, and summary. Use `api operation get <operationId>` \
                 to inspect a result in full detail.",
            )
            .with_system("api")
            .with_tier(Tier::Read)
            .no_auth(true)
            .with_default_fields("domain,method,path,summary")
            .with_output_schema::<ApiEndpoint>(),
        |_cred, args: SearchArgs| async move {
            let hits = search_endpoints(catalog(), &args.query);
            if hits.is_empty() {
                return Ok(CommandResult::new(json!([])));
            }
            let results: Vec<Value> = hits
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
            Ok(CommandResult::new(json!(results)).with_next_actions(vec![
                next_action(
                    "api operation get <operation>",
                    "Get full details for a result",
                )
                .with_param("operation", NextActionParam::required()),
            ]))
        },
    )
}
