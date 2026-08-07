//! `api response list` and `api response get`.

use cli_engine::{
    CommandResult, CommandSpec, NextActionParam, PaginationConfig, RuntimeCommandSpec, TableColumn,
    Tier,
};
use serde_json::json;

use crate::next_action::{next_action, required_value};

use super::catalog::{catalog, resolve_operation};
use super::summary::{
    OPERATION_CHILD_LIST_DEFAULT_LIMIT, OPERATION_CHILD_LIST_MAX_LIMIT, response_rows, to_json,
};

#[derive(Debug, Clone, clap::Args)]
struct ResponseListArgs {
    /// Operation ID to list responses for (see `api operation list`).
    #[arg(long, value_name = "OPERATION")]
    operation: String,
}

pub(super) fn list_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed::<ResponseListArgs, _, _, _>(
        CommandSpec::from_args::<ResponseListArgs>("list", "List an operation's responses")
            .with_long(
                "Lists every response on one operation by status code. Use \
                `api response get <status> --operation <id>` for one response's \
                full detail.",
            )
            .with_view(vec![
                TableColumn::new("status", "Status"),
                TableColumn::new("type", "Type"),
                TableColumn::new("schemaId", "Schema ID"),
                TableColumn::new("description", "Description"),
            ])
            .with_system("api")
            .with_tier(Tier::Read)
            .no_auth(true)
            .with_pagination(PaginationConfig {
                default_limit: OPERATION_CHILD_LIST_DEFAULT_LIMIT as i64,
                max_limit: OPERATION_CHILD_LIST_MAX_LIMIT,
            }),
        |_cred, args: ResponseListArgs| async move {
            let operation = args.operation.as_str();
            let catalog = catalog();
            let (domain, ep) = resolve_operation(catalog, operation, None)?;
            let all = response_rows(ep, &domain.defs);
            let next_actions = vec![
                next_action(
                    "api response get <status> --operation <operation>",
                    "See one response's full detail",
                )
                .with_param("status", NextActionParam::required())
                .with_param("operation", required_value(ep.operation_id.clone())),
            ];
            Ok(CommandResult::new(json!(all)).with_next_actions(next_actions))
        },
    )
}

#[derive(Debug, Clone, clap::Args)]
struct ResponseGetArgs {
    /// HTTP status code (e.g. 200).
    #[arg(value_name = "STATUS")]
    status: String,

    /// Operation ID that owns this response (see `api operation list`).
    #[arg(long, value_name = "OPERATION")]
    operation: String,
}

pub(super) fn get_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed::<ResponseGetArgs, _, _, _>(
        CommandSpec::from_args::<ResponseGetArgs>(
            "get",
            "Show one operation response's full detail",
        )
        .with_long(
            "Shows one response's full detail. Use `api schema get` for complete \
            details on complex response schemas.",
        )
        .with_system("api")
        .with_tier(Tier::Read)
        .no_auth(true),
        |_cred, args: ResponseGetArgs| async move {
            let status = args.status.as_str();
            let operation = args.operation.as_str();
            let catalog = catalog();
            let (domain, ep) = resolve_operation(catalog, operation, None)?;
            let row = response_rows(ep, &domain.defs)
                .into_iter()
                .find(|r| r.status == status)
                .ok_or_else(|| {
                    crate::error::GddyError::not_found(format!(
                        "response '{status}' not found on operation '{}'",
                        ep.operation_id
                    ))
                    .with_fix(format!(
                        "Run: gddy api response list --operation {}",
                        ep.operation_id
                    ))
                    .into_cli_error()
                })?;
            let next_actions = row
                .schema_id
                .clone()
                .map(|id| {
                    next_action("api schema get <id>", "See the full response schema")
                        .with_param("id", required_value(id))
                })
                .into_iter()
                .collect();
            Ok(CommandResult::new(to_json(row)?).with_next_actions(next_actions))
        },
    )
}

#[cfg(test)]
mod tests {
    use cli_engine::{Cli, CliConfig};
    use serde_json::json;

    fn operation_cli() -> Cli {
        Cli::new(
            CliConfig::new("gddy", "GoDaddy developer CLI", "gddy")
                .with_module(super::super::module())
                .with_environments(std::sync::Arc::clone(crate::environments::instance())),
        )
    }

    /// Same regression as `parameter::tests::parameter_list_human_output_uses_the_declared_column_order`,
    /// for `response_list_command`. `createShipment`'s rows (long
    /// descriptions, a long dotted schema id) push the declared
    /// `Description` column past the default terminal width, so it's the
    /// lowest-priority column cli-engine drops to make everything else fit
    /// — the header below is missing it for that reason, not because the
    /// view only has three columns. Assert the drop is reported, so this
    /// doesn't read as `Description` being absent from the view too.
    #[tokio::test]
    async fn response_list_human_output_uses_the_declared_column_order() {
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "response",
                "list",
                "--operation",
                "createShipment",
                "--output",
                "human",
            ])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let header_line = output
            .rendered
            .lines()
            .next()
            .expect("at least a header line")
            .trim_end();
        assert_eq!(
            header_line, "STATUS  TYPE    SCHEMA ID",
            "{}",
            output.rendered
        );
        assert!(
            output
                .rendered
                .contains("hidden to fit the display width (Description)"),
            "Description should be declared but dropped for width, not absent from the view: {}",
            output.rendered
        );
    }

    #[tokio::test]
    async fn response_list_returns_every_status_sorted() {
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "response",
                "list",
                "--operation",
                "patchBusiness",
                "--output",
                "json",
            ])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let rendered: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json");
        let items = rendered["data"].as_array().expect("data array");
        let statuses: Vec<&str> = items
            .iter()
            .map(|r| r["status"].as_str().expect("status"))
            .collect();
        assert_eq!(statuses, vec!["200", "404", "default"]);
    }

    #[tokio::test]
    async fn response_get_returns_one_status_with_its_schema_preview() {
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "response",
                "get",
                "200",
                "--operation",
                "commerce.location.verify-address",
                "--output",
                "json",
            ])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let rendered: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json");
        assert_eq!(rendered["data"]["status"], json!("200"));
        let schema_items = rendered["data"]["schema"]["items"]
            .as_array()
            .expect("schema items array");
        let names: Vec<&str> = schema_items
            .iter()
            .map(|p| p["name"].as_str().expect("name"))
            .collect();
        assert!(names.contains(&"status"));
        assert!(names.contains(&"data"));
        assert_eq!(rendered["data"]["type"], json!("object"));
        let schema_id = rendered["data"]["schemaId"]
            .as_str()
            .expect("schemaId present whenever a schema exists");
        let next_actions = rendered["next_actions"].as_array().expect("next_actions");
        let link = next_actions
            .iter()
            .find(|a| a["command"].as_str().unwrap_or("").contains("schema get"))
            .expect("schema next_action attached even though the preview isn't truncated");
        assert_eq!(link["params"]["id"]["value"], json!(schema_id));
    }

    #[tokio::test]
    async fn response_get_unknown_status_is_a_not_found_error() {
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "response",
                "get",
                "599",
                "--operation",
                "commerce.location.verify-address",
                "--output",
                "json",
            ])
            .await;
        assert_ne!(output.exit_code, 0, "{}", output.rendered);
        assert!(output.rendered.contains("not found"), "{}", output.rendered);
    }
}
