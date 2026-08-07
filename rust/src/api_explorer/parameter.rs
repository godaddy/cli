//! `api parameter list` and `api parameter get`.

use cli_engine::{
    CommandResult, CommandSpec, NextActionParam, PaginationConfig, RuntimeCommandSpec, TableColumn,
    Tier,
};
use serde_json::json;

use crate::next_action::{next_action, required_value};

use super::catalog::{catalog, resolve_operation};
use super::summary::{
    OPERATION_CHILD_LIST_DEFAULT_LIMIT, OPERATION_CHILD_LIST_MAX_LIMIT, summarize_parameters,
    to_json,
};

#[derive(Debug, Clone, clap::Args)]
struct ParameterListArgs {
    /// Operation ID to list parameters for (see `api operation list`).
    #[arg(long, value_name = "OPERATION")]
    operation: String,
}

pub(super) fn list_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed::<ParameterListArgs, _, _, _>(
        CommandSpec::from_args::<ParameterListArgs>("list", "List an operation's parameters")
            .with_long(
                "Summarizes every parameter for an operation — query/path/header/cookie/body. \
                Use `api parameter get <name> --operation <id>` for a \
                parameter's full details.",
            )
            .with_view(vec![
                TableColumn::new("name", "Name"),
                TableColumn::new("in", "In"),
                TableColumn::new("required", "Required"),
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
        |_cred, args: ParameterListArgs| async move {
            let operation = args.operation.as_str();
            let catalog = catalog();
            let (domain, ep) = resolve_operation(catalog, operation, None)?;
            let all = summarize_parameters(ep, &domain.defs);
            let next_actions = vec![
                next_action(
                    "api parameter get <name> --operation <operation>",
                    "See one parameter's full detail",
                )
                .with_param("name", NextActionParam::required())
                .with_param("operation", required_value(ep.operation_id.clone())),
            ];
            Ok(CommandResult::new(json!(all)).with_next_actions(next_actions))
        },
    )
}

#[derive(Debug, Clone, clap::Args)]
struct ParameterGetArgs {
    /// Parameter name (or `body` for the request body, if present).
    #[arg(value_name = "NAME")]
    name: String,

    /// Operation ID that owns this parameter (see `api operation list`).
    #[arg(long, value_name = "OPERATION")]
    operation: String,
}

pub(super) fn get_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed::<ParameterGetArgs, _, _, _>(
        CommandSpec::from_args::<ParameterGetArgs>(
            "get",
            "Show an operation parameter's full details",
        )
        .with_long(
            "Shows parameter details, such as location, required-ness, \
            description. Use `api schema get` for complete details of a parameter \
            with a complex data type.",
        )
        .with_system("api")
        .with_tier(Tier::Read)
        .no_auth(true),
        |_cred, args: ParameterGetArgs| async move {
            let name = args.name.as_str();
            let operation = args.operation.as_str();
            let catalog = catalog();
            let (domain, ep) = resolve_operation(catalog, operation, None)?;
            let row = summarize_parameters(ep, &domain.defs)
                .into_iter()
                .find(|p| p.name == name)
                .ok_or_else(|| {
                    crate::error::GddyError::not_found(format!(
                        "parameter '{name}' not found on operation '{}'",
                        ep.operation_id
                    ))
                    .with_fix(format!(
                        "Run: gddy api parameter list --operation {}",
                        ep.operation_id
                    ))
                    .into_cli_error()
                })?;
            let next_actions = row
                .schema_id
                .clone()
                .map(|id| {
                    next_action("api schema get <id>", "See the full parameter schema")
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

    #[tokio::test]
    async fn parameter_list_folds_in_the_synthetic_body_row() {
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "parameter",
                "list",
                "--operation",
                "commerce.location.verify-address",
                "--output",
                "json",
            ])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let rendered: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json");
        let items = rendered["data"].as_array().expect("data array");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["name"], json!("body"));
        assert_eq!(items[0]["in"], json!("body"));
        assert_eq!(items[0]["required"], json!(true));
    }

    #[tokio::test]
    async fn parameter_list_human_output_uses_the_declared_column_order() {
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "parameter",
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
            header_line, "NAME     IN    REQUIRED  TYPE    SCHEMA ID  DESCRIPTION",
            "{}",
            output.rendered
        );
    }

    #[tokio::test]
    async fn parameter_list_honors_engine_provided_limit_and_offset() {
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "parameter",
                "list",
                "--operation",
                "get_transaction_disputes",
                "--limit",
                "5",
                "--offset",
                "20",
                "--output",
                "json",
            ])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let rendered: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json");
        assert_eq!(rendered["data"].as_array().expect("data array").len(), 5);
        let pagination = &rendered["pagination"];
        assert_eq!(pagination["total"], json!(26));
        assert_eq!(pagination["offset"], json!(20));
        assert_eq!(pagination["limit"], json!(5));
        assert_eq!(pagination["count"], json!(5));
        // 20 + 5 = 25 < 26, so one more remains.
        assert_eq!(pagination["has_more"], json!(true));
        let next_actions = rendered["next_actions"].as_array().expect("next_actions");
        assert!(
            next_actions
                .iter()
                .any(|a| a["command"].as_str().unwrap_or("").contains("--offset 25")),
            "expected an auto-generated next-page hint: {}",
            output.rendered
        );
    }

    /// `--limit 0` means "all" per cli-engine's own generated help text for
    /// `with_pagination` — must show every item, not zero. With both
    /// `--limit`/`--offset` at their "disabled" value (0), cli-engine 0.8's
    /// pipeline skips pagination entirely rather than reporting a page of
    /// everything, so there's no `pagination` envelope field at all here.
    #[tokio::test]
    async fn parameter_list_limit_zero_means_all() {
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "parameter",
                "list",
                "--operation",
                "get_transaction_disputes",
                "--limit",
                "0",
                "--output",
                "json",
            ])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let rendered: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json");
        assert_eq!(rendered["data"].as_array().expect("data array").len(), 26);
        assert!(rendered.get("pagination").is_none(), "{}", output.rendered);
    }

    /// `max_limit` (`OPERATION_CHILD_LIST_MAX_LIMIT`) rejects an
    /// out-of-range `--limit` at parse time, before the handler even runs.
    #[tokio::test]
    async fn parameter_list_limit_above_max_is_rejected() {
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "parameter",
                "list",
                "--operation",
                "get_transaction_disputes",
                "--limit",
                "999",
                "--output",
                "json",
            ])
            .await;
        assert_ne!(output.exit_code, 0, "{}", output.rendered);
    }

    #[tokio::test]
    async fn parameter_get_resolves_the_synthetic_body_name() {
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "parameter",
                "get",
                "body",
                "--operation",
                "commerce.location.verify-address",
                "--output",
                "json",
            ])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let rendered: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json");
        assert_eq!(rendered["data"]["name"], json!("body"));
        assert_eq!(rendered["data"]["in"], json!("body"));
        assert!(rendered["data"]["schema"]["items"].is_array());
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

    /// `storeId` on `queryCarriers` has no `schema` at all — the id and its
    /// "see full schema" next_action should both be absent, not present with
    /// an empty/garbage value.
    #[tokio::test]
    async fn parameter_get_without_a_schema_has_no_schema_id_or_next_action() {
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "parameter",
                "get",
                "storeId",
                "--operation",
                "queryCarriers",
                "--output",
                "json",
            ])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let rendered: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json");
        assert!(rendered["data"]["schemaId"].is_null());
        assert!(
            rendered["next_actions"]
                .as_array()
                .map(|a| a.is_empty())
                .unwrap_or(true),
            "{}",
            output.rendered
        );
    }

    /// `pageSize` on `queryCarriers` has a bare inline scalar schema
    /// (`{"type": "integer", "format": "int64", ...}`, no `properties`, no
    /// `$ref`) — there's nothing to drill into, so it should surface `type`
    /// directly rather than a `schemaId`/next_action pointing at a
    /// `schema get` that would just echo the same type back.
    #[tokio::test]
    async fn parameter_get_scalar_schema_shows_type_instead_of_schema_id() {
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "parameter",
                "get",
                "pageSize",
                "--operation",
                "queryCarriers",
                "--output",
                "json",
            ])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let rendered: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json");
        assert!(rendered["data"]["schemaId"].is_null());
        assert_eq!(rendered["data"]["type"], json!("integer(int64)"));
        assert!(
            rendered["next_actions"]
                .as_array()
                .map(|a| a.is_empty())
                .unwrap_or(true),
            "{}",
            output.rendered
        );
    }

    #[tokio::test]
    async fn parameter_get_unknown_name_is_a_not_found_error() {
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "parameter",
                "get",
                "does-not-exist",
                "--operation",
                "commerce.location.verify-address",
                "--output",
                "json",
            ])
            .await;
        assert_ne!(output.exit_code, 0, "{}", output.rendered);
        assert!(output.rendered.contains("not found"), "{}", output.rendered);
    }

    /// `getChannels`'s `registeredStores.storeId` parameter has a literal
    /// dot in its name — a real edge case for the schema-id grammar (see
    /// `schema_id_round_trip_holds_across_the_real_catalog` in
    /// `super::super::schema`), exercised here end-to-end through the actual
    /// command.
    #[tokio::test]
    async fn parameter_get_handles_a_dotted_parameter_name() {
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "parameter",
                "get",
                "registeredStores.storeId",
                "--operation",
                "getChannels",
                "--output",
                "json",
            ])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let rendered: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json");
        assert_eq!(rendered["data"]["name"], json!("registeredStores.storeId"));
    }
}
