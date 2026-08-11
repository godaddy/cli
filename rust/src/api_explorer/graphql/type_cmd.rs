//! `api graphql type get` — a nested group with a single leaf, kept in one
//! flat file rather than its own subdirectory (see `docs/code-structure.md`).
//! Named `type_cmd` rather than `type` since `type` is a Rust keyword.

use cli_engine::{
    CommandResult, CommandSpec, GroupSpec, RuntimeCommandSpec, RuntimeGroupSpec, TableColumn, Tier,
};
use serde_json::json;

use crate::output_schema::output_schema;

use super::super::catalog::{
    ambiguous_graphql_type_error, catalog, find_graphql_types, graphql_type_not_found_error,
};

output_schema!(ApiGraphqlType {
    "name": "string";
    "kind": "string";
    "fields": "[]object", optional;
    "values": "[]string", optional;
});

#[derive(Debug, Clone, clap::Args)]
struct GraphqlTypeGetArgs {
    /// GraphQL type name (see a field's `Type` column in `api graphql get`).
    #[arg(value_name = "NAME")]
    name: String,

    /// Domain to look up the type in, when the name is defined by more than
    /// one domain's GraphQL schema (see `api domain list`).
    #[arg(long, value_name = "DOMAIN")]
    domain: Option<String>,
}

fn get_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed::<GraphqlTypeGetArgs, _, _, _>(
        CommandSpec::from_args::<GraphqlTypeGetArgs>(
            "get",
            "Show a named GraphQL type's fields or values",
        )
        .with_long(
            "Looks up a GraphQL object, input, or enum type by name and shows its full field \
             list (or, for an enum, its allowed values) — never truncated. Use this to drill \
             into a field whose `Type` column in `api graphql get` isn't a plain scalar (e.g. \
             `ClassificationListsConnection`), one hop at a time, until you reach the fields \
             you want to pass to `--select`.",
        )
        .with_system("api")
        .with_tier(Tier::Read)
        .no_auth(true)
        .with_output_schema::<ApiGraphqlType>()
        .with_view(vec![
            TableColumn::new("name", "Name"),
            TableColumn::new("kind", "Kind"),
            TableColumn::new("fields", "Fields").nested(vec![
                TableColumn::new("name", "Name"),
                TableColumn::new("type", "Type"),
                TableColumn::new("description", "Description"),
            ]),
            TableColumn::new("values", "Values"),
        ]),
        |_cred, args: GraphqlTypeGetArgs| async move {
            let name = args.name.as_str();
            let matches = find_graphql_types(catalog(), name);
            let t = match args.domain.as_deref() {
                Some(domain) => matches
                    .iter()
                    .find(|(d, _)| d.name == domain)
                    .map(|(_, t)| *t)
                    .ok_or_else(|| graphql_type_not_found_error(name))?,
                None => match matches.as_slice() {
                    [] => return Err(graphql_type_not_found_error(name)),
                    [(_, t)] => *t,
                    hits => {
                        let domains: Vec<&str> =
                            hits.iter().map(|(d, _)| d.name.as_str()).collect();
                        return Err(ambiguous_graphql_type_error(name, &domains));
                    }
                },
            };

            let fields = (!t.fields.is_empty()).then(|| {
                t.fields
                    .iter()
                    .map(|f| {
                        json!({"name": f.name, "type": f.field_type, "description": f.description})
                    })
                    .collect::<Vec<_>>()
            });
            let values = (!t.values.is_empty()).then(|| t.values.clone());

            Ok(CommandResult::new(json!({
                "name": t.name,
                "kind": t.kind,
                "fields": fields,
                "values": values,
            })))
        },
    )
}

pub(super) fn group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(GroupSpec::new(
        "type",
        "Look up a named GraphQL object, input, or enum type",
    ))
    .with_command(get_command())
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

    /// Finds a named object/input type with at least one field, and a named
    /// enum type, on the taxes GraphQL schema — used instead of hardcoding
    /// type names an upstream spec change could rename.
    fn an_object_and_enum_type_name() -> (String, String) {
        let (_, ep) = super::super::super::catalog::find_endpoint(
            super::super::super::catalog::catalog(),
            "postTaxGraphql",
        )
        .expect("postTaxGraphql exists in the embedded taxes catalog");
        let schema = ep
            .graphql
            .as_ref()
            .expect("postTaxGraphql has a graphql schema");
        let object_name = schema
            .types
            .iter()
            .find(|t| t.kind != "enum" && !t.fields.is_empty())
            .expect("taxes graphql schema has an object/input type with fields")
            .name
            .clone();
        let enum_name = schema
            .types
            .iter()
            .find(|t| t.kind == "enum" && !t.values.is_empty())
            .expect("taxes graphql schema has an enum type with values")
            .name
            .clone();
        (object_name, enum_name)
    }

    #[tokio::test]
    async fn graphql_type_get_returns_fields_for_an_object_type() {
        let (object_name, _) = an_object_and_enum_type_name();
        let output = graphql_cli()
            .run([
                "gddy",
                "api",
                "graphql",
                "type",
                "get",
                &object_name,
                "--domain",
                "taxes",
                "--output",
                "json",
            ])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let rendered: Value = serde_json::from_str(&output.rendered).expect("valid json");
        assert!(
            !rendered["data"]["fields"]
                .as_array()
                .expect("fields array")
                .is_empty()
        );
    }

    #[tokio::test]
    async fn graphql_type_get_returns_values_for_an_enum_type() {
        let (_, enum_name) = an_object_and_enum_type_name();
        let output = graphql_cli()
            .run([
                "gddy", "api", "graphql", "type", "get", &enum_name, "--domain", "taxes",
                "--output", "json",
            ])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let rendered: Value = serde_json::from_str(&output.rendered).expect("valid json");
        assert!(
            !rendered["data"]["values"]
                .as_array()
                .expect("values array")
                .is_empty()
        );
    }

    /// Regression for a name collision caught in PR #200 review: `taxes`
    /// and `catalog-products` each declare their own, differently-shaped
    /// `ReferenceValueFilter` input type. Without `--domain`, that must be
    /// an ambiguous-match error rather than silently returning whichever
    /// domain happens to sort first — and each `--domain` value must return
    /// that domain's own fields, not the other's.
    #[tokio::test]
    async fn graphql_type_get_disambiguates_a_name_shared_by_two_domains() {
        let ambiguous = graphql_cli()
            .run([
                "gddy",
                "api",
                "graphql",
                "type",
                "get",
                "ReferenceValueFilter",
                "--output",
                "json",
            ])
            .await;
        assert_ne!(ambiguous.exit_code, 0, "{}", ambiguous.rendered);
        assert!(ambiguous.rendered.contains("taxes"));
        assert!(ambiguous.rendered.contains("catalog-products"));

        let taxes = graphql_cli()
            .run([
                "gddy",
                "api",
                "graphql",
                "type",
                "get",
                "ReferenceValueFilter",
                "--domain",
                "taxes",
                "--output",
                "json",
            ])
            .await;
        assert_eq!(taxes.exit_code, 0, "{}", taxes.rendered);
        let taxes_fields: Value = serde_json::from_str(&taxes.rendered).expect("valid json");
        let taxes_field_names: Vec<&str> = taxes_fields["data"]["fields"]
            .as_array()
            .expect("fields array")
            .iter()
            .map(|f| f["name"].as_str().expect("field name"))
            .collect();
        assert_eq!(taxes_field_names, ["in"]);

        let catalog_products = graphql_cli()
            .run([
                "gddy",
                "api",
                "graphql",
                "type",
                "get",
                "ReferenceValueFilter",
                "--domain",
                "catalog-products",
                "--output",
                "json",
            ])
            .await;
        assert_eq!(
            catalog_products.exit_code, 0,
            "{}",
            catalog_products.rendered
        );
        let cp_fields: Value =
            serde_json::from_str(&catalog_products.rendered).expect("valid json");
        let cp_field_names: Vec<&str> = cp_fields["data"]["fields"]
            .as_array()
            .expect("fields array")
            .iter()
            .map(|f| f["name"].as_str().expect("field name"))
            .collect();
        assert_eq!(cp_field_names, ["eq", "in"]);
    }

    #[tokio::test]
    async fn graphql_type_get_unknown_name_is_not_found() {
        let output = graphql_cli()
            .run([
                "gddy",
                "api",
                "graphql",
                "type",
                "get",
                "NotARealGraphqlType",
                "--output",
                "json",
            ])
            .await;
        assert_ne!(output.exit_code, 0, "{}", output.rendered);
    }
}
