//! `api search`.

use cli_engine::{
    CommandResult, CommandSpec, CursorConfig, NextActionParam, RuntimeCommandSpec, Tier,
};
use serde_json::{Value, json};

use crate::next_action::next_action;
use crate::output_schema::output_schema;
use crate::pagination_token;

use super::catalog::{catalog, graphql_sub_endpoint_rows, search_endpoints};

/// Largest page `api search` returns (its cursor `max_limit`).
const MAX_LIMIT: i64 = 100;

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

    /// Jump directly to this offset instead of walking there one --continue
    /// token at a time. Ignored when `--continue` is given.
    #[arg(long, value_name = "N", value_parser = clap::value_parser!(i64).range(0..))]
    offset: Option<i64>,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<SearchArgs, _, _, _>(
        CommandSpec::from_args::<SearchArgs>("search", "Search API endpoints by keyword")
            .with_long(
                "Full-text searches across all API domains, matching against \
                operation IDs, paths, summaries, and descriptions. Use \
                `api operation get <operationId>` to inspect a result in full \
                detail.",
            )
            .with_system("api")
            .with_tier(Tier::Read)
            .no_auth(true)
            .with_default_fields("operationId,domain,method,path,summary")
            .with_output_schema::<ApiEndpoint>()
            .with_cursor(CursorConfig::new(50, MAX_LIMIT)),
        |ctx, args: SearchArgs| async move {
            let hits = search_endpoints(catalog(), &args.query);
            if hits.is_empty() {
                let (page, continuation) = pagination_token::paginate_known_collection(
                    Vec::new(),
                    ctx.middleware.cursor_limit,
                    MAX_LIMIT,
                    None,
                )?;
                return Ok(CommandResult::new(json!(page)).with_cursor(continuation));
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

            let continue_token = pagination_token::resolve_continue_token(
                args.offset,
                ctx.middleware.continue_token.as_deref(),
            );
            let (page, continuation) = pagination_token::paginate_known_collection(
                pagination_token::filter_known_collection(results, &ctx.middleware.filter)?,
                ctx.middleware.cursor_limit,
                MAX_LIMIT,
                continue_token.as_deref(),
            )?;
            Ok(CommandResult::new(json!(page))
                .with_cursor(continuation)
                .with_next_actions(next_actions))
        },
    )
}

#[cfg(test)]
mod tests {
    use cli_engine::{Cli, CliConfig, CursorConfig, Stage};

    use super::command;

    #[test]
    fn command_opts_into_cursor_pagination_with_a_default_and_a_max_limit() {
        assert_eq!(command().spec.cursor, Some(CursorConfig::new(50, 100)));
    }

    fn test_cli() -> Cli {
        Cli::new(
            CliConfig::new("gddy", "GoDaddy developer CLI", "gddy")
                .with_min_stage(Stage::Experimental)
                .with_modules(crate::all_modules()),
        )
    }

    #[tokio::test]
    async fn limit_and_continue_page_through_search_results() {
        let cli = test_cli();
        // "list" matches many operation IDs/paths/summaries across the
        // embedded catalog — plenty of hits to page through.
        let first = cli
            .run([
                "gddy", "api", "search", "list", "--limit", "1", "--output", "json",
            ])
            .await;
        assert_eq!(first.exit_code, 0, "{}", first.rendered);
        let first_json: serde_json::Value =
            serde_json::from_str(&first.rendered).expect("stdout should contain json");
        assert_eq!(first_json["data"].as_array().expect("array").len(), 1);
        assert_eq!(first_json["cursor"]["has_more"], serde_json::json!(true));
        let token = first_json["cursor"]["continue_from"]
            .as_str()
            .expect("has_more implies a token")
            .to_owned();

        let second = cli
            .run([
                "gddy",
                "api",
                "search",
                "list",
                "--limit",
                "1",
                "--continue",
                &token,
                "--output",
                "json",
            ])
            .await;
        assert_eq!(second.exit_code, 0, "{}", second.rendered);
        let second_json: serde_json::Value =
            serde_json::from_str(&second.rendered).expect("stdout should contain json");

        let via_offset = cli
            .run([
                "gddy", "api", "search", "list", "--limit", "1", "--offset", "1", "--output",
                "json",
            ])
            .await;
        assert_eq!(via_offset.exit_code, 0, "{}", via_offset.rendered);
        let via_offset_json: serde_json::Value =
            serde_json::from_str(&via_offset.rendered).expect("stdout should contain json");
        assert_eq!(via_offset_json["data"], second_json["data"]);
    }
}
