//! `api graphql sdl get` — a nested group with a single leaf, kept in one
//! flat file rather than its own subdirectory (see `docs/code-structure.md`).

use cli_engine::{
    CommandResult, CommandSpec, GroupSpec, RuntimeCommandSpec, RuntimeGroupSpec, Tier,
};
use serde_json::json;

use super::super::catalog::{catalog, resolve_operation};

#[derive(Debug, Clone, clap::Args)]
struct GraphqlSdlGetArgs {
    /// The GraphQL domain's wrapper operation ID, not an individual
    /// query/mutation id.
    #[arg(value_name = "OPERATION")]
    operation: String,
}

fn get_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed::<GraphqlSdlGetArgs, _, _, _>(
        CommandSpec::from_args::<GraphqlSdlGetArgs>(
            "get",
            "Show a GraphQL domain's actual schema (SDL) text",
        )
        .with_long(
            "Prints the GraphQL Schema Definition Language source for a GraphQL \
            domain. Useful for feeding into other GraphQL tooling or for \
            answering any question this catalog's own summary doesn't happen to \
            capture. Pass the wrapper operation id for the domain (e.g. \
            `postTaxGraphql`), not an individual query/mutation id.",
        )
        .with_system("api")
        .with_tier(Tier::Read)
        .no_auth(true)
        .raw_output(true),
        |_cred, args: GraphqlSdlGetArgs| async move {
            let operation = args.operation.as_str();
            let (_, ep) = resolve_operation(catalog(), operation, None)?;
            let schema = ep.graphql.as_ref().ok_or_else(|| {
                crate::error::GddyError::validation(format!(
                    "'{operation}' has no GraphQL schema attached — pass a domain's wrapper \
                     operation id instead, e.g. postTaxGraphql"
                ))
                .into_cli_error()
            })?;
            Ok(CommandResult::new(json!(schema.sdl)))
        },
    )
}

pub(super) fn group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(GroupSpec::new(
        "sdl",
        "Get a GraphQL domain's actual schema text, verbatim",
    ))
    .with_command(get_command())
}

#[cfg(test)]
mod tests {
    use cli_engine::{Cli, CliConfig};

    fn graphql_cli() -> Cli {
        Cli::new(
            CliConfig::new("gddy", "GoDaddy developer CLI", "gddy")
                .with_module(crate::api_explorer::module()),
        )
    }

    #[tokio::test]
    async fn graphql_sdl_get_always_prints_raw_text_by_default() {
        let output = graphql_cli()
            .run(["gddy", "api", "graphql", "sdl", "get", "postTaxGraphql"])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        assert!(
            output.rendered.contains("type ") || output.rendered.contains("schema"),
            "expected raw SDL text, got: {}",
            output.rendered
        );
        assert!(
            serde_json::from_str::<serde_json::Value>(&output.rendered).is_err(),
            "expected raw text, not JSON: {}",
            output.rendered
        );
    }

    #[tokio::test]
    async fn graphql_sdl_get_ignores_an_explicit_output_json_flag() {
        let output = graphql_cli()
            .run([
                "gddy",
                "api",
                "graphql",
                "sdl",
                "get",
                "postTaxGraphql",
                "--output",
                "json",
            ])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        assert!(
            serde_json::from_str::<serde_json::Value>(&output.rendered).is_err(),
            "expected raw text even with --output json, got: {}",
            output.rendered
        );
    }

    #[tokio::test]
    async fn graphql_sdl_get_rejects_an_operation_with_no_graphql_schema() {
        let output = graphql_cli()
            .run(["gddy", "api", "graphql", "sdl", "get", "listFulfillments"])
            .await;
        assert_ne!(output.exit_code, 0, "{}", output.rendered);
    }
}
