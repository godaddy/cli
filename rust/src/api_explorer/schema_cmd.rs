//! `api schema get`.

use cli_engine::{CommandResult, CommandSpec, RuntimeCommandSpec, Tier};

use super::catalog::{catalog, resolve_operation};
use super::schema_tree::{SchemaLocation, build_schema_tree, parse_schema_id};
use super::summary::{find_parameter_schema, to_json};

#[derive(Debug, Clone, clap::Args)]
struct SchemaGetArgs {
    /// Schema id — a component name, or a dotted path from a truncated preview.
    #[arg(value_name = "ID")]
    id: String,
}

pub(super) fn get_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed::<SchemaGetArgs, _, _, _>(
        CommandSpec::from_args::<SchemaGetArgs>("get", "Show a schema's full nested tree")
            .with_long(
                "Shows the complete nested structure of a schema — every property at every \
                 level, with $ref's resolved inline. This is the terminal drill-down target \
                 for a truncated schema preview shown elsewhere.",
            )
            .with_system("api")
            .with_tier(Tier::Read)
            .no_auth(true),
        |_cred, args: SchemaGetArgs| async move {
            let id = args.id.as_str();
            let catalog = catalog();

            // A bare component name (no schema-id keyword segment) is looked
            // up directly against every domain's `$defs` — it isn't scoped
            // to one operation the way an inline dotted id is.
            if let Some((domain_defs, schema)) = catalog
                .iter()
                .find_map(|d| d.defs.get(id).map(|schema| (&d.defs, schema)))
            {
                let tree = build_schema_tree(id, schema, domain_defs);
                return Ok(CommandResult::new(to_json(tree)?));
            }

            let (operation_id, location) = parse_schema_id(id).ok_or_else(|| {
                crate::error::GddyError::not_found(format!("no schema found for id '{id}'"))
                    .with_fix("Run: gddy api operation get <operationId> to find schema ids")
                    .into_cli_error()
            })?;

            let (domain, ep) = resolve_operation(catalog, &operation_id, None)?;

            let raw_schema = match &location {
                SchemaLocation::RequestBody => {
                    ep.request_body.as_ref().and_then(|b| b.get("schema"))
                }
                SchemaLocation::Parameter(name) => {
                    find_parameter_schema(ep, name).map(|(schema, _)| schema)
                }
                SchemaLocation::Response(status) => ep
                    .responses
                    .get(status.as_str())
                    .and_then(|r| r.get("schema")),
            };
            let raw_schema = raw_schema.ok_or_else(|| {
                crate::error::GddyError::not_found(format!("no schema found for id '{id}'"))
                    .into_cli_error()
            })?;

            let tree = build_schema_tree(id, raw_schema, &domain.defs);
            Ok(CommandResult::new(to_json(tree)?))
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

    /// A `$ref` schema's id is its bare `$defs` key — no operation scoping
    /// needed to look it up.
    #[tokio::test]
    async fn schema_get_resolves_a_bare_defs_name() {
        let output = operation_cli()
            .run([
                "gddy", "api", "schema", "get", "address", "--output", "json",
            ])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let rendered: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json");
        assert_eq!(rendered["data"]["type"], json!("object"));
        let properties = rendered["data"]["properties"]
            .as_array()
            .expect("properties array");
        assert!(properties.iter().any(|p| p["name"] == "addressDetails"));
    }

    /// An inline response schema's id is a dotted path rooted at the
    /// operationId — which, for this operation, itself contains dots, so
    /// this also exercises the schema-id parser's keyword-scan (not a fixed
    /// split position).
    #[tokio::test]
    async fn schema_get_resolves_an_inline_response_schema_by_dotted_id() {
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "schema",
                "get",
                "commerce.location.verify-address.responses.200.schema",
                "--output",
                "json",
            ])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let rendered: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json");
        assert_eq!(rendered["data"]["type"], json!("object"));
        let properties = rendered["data"]["properties"]
            .as_array()
            .expect("properties array");
        let names: Vec<&str> = properties
            .iter()
            .map(|p| p["name"].as_str().expect("name"))
            .collect();
        assert!(names.contains(&"status"));
        assert!(names.contains(&"data"));
    }

    /// An inline request body's id is a dotted path too — `patchBusiness`'s
    /// top-level schema is `{type: array, items: {$ref: ...}}`, inline at
    /// the top level even though its items aren't.
    #[tokio::test]
    async fn schema_get_resolves_an_inline_request_body_schema_by_dotted_id() {
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "schema",
                "get",
                "patchBusiness.requestBody.schema",
                "--output",
                "json",
            ])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let rendered: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json");
        assert_eq!(rendered["data"]["type"], json!("array"));
        assert!(rendered["data"]["items"].is_object());
    }

    #[tokio::test]
    async fn schema_get_unknown_id_is_a_not_found_error() {
        let output = operation_cli()
            .run([
                "gddy",
                "api",
                "schema",
                "get",
                "not-a-defs-name-and-not-a-dotted-id",
                "--output",
                "json",
            ])
            .await;
        assert_ne!(output.exit_code, 0, "{}", output.rendered);
        assert!(
            output.rendered.contains("no schema found for id"),
            "{}",
            output.rendered
        );
    }
}
