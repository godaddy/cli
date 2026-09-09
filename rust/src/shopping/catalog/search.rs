use cli_engine::{CommandResult, CommandSpec, RuntimeCommandSpec, Tier};

use crate::output_schema::output_schema;
use crate::shopping::SHOPPING_SCOPES;
use crate::shopping::common::{client_err, make_client, read_json};

output_schema!(CatalogSearchOutput {
    "ucp": "object";
    "products": "[]object";
    "pagination": "object", optional;
    "messages": "[]object";
});

#[derive(Debug, Clone, clap::Args)]
struct Args {
    /// Search request as raw JSON. Use `{}` to browse all products.
    #[arg(long, value_name = "JSON", required_unless_present = "file")]
    body: Option<String>,

    /// Path to a JSON search request. Takes precedence over --body.
    #[arg(long, value_name = "PATH")]
    file: Option<String>,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<Args, _, _, _>(
        CommandSpec::from_args::<Args>("search", "Search the Shopping catalog")
            .with_long("Submit a UCP catalog-search JSON object. Use `{}` to browse all products.")
            .with_system("shopping")
            .with_tier(Tier::Read)
            .with_scopes(SHOPPING_SCOPES)
            .with_output_schema::<CatalogSearchOutput>()
            .with_default_fields("products,pagination,messages"),
        |ctx, args: Args| async move {
            let body = read_json(args.body.as_deref(), args.file.as_deref(), "object")?;
            let client = make_client(&ctx).await?;
            Ok(CommandResult::new(
                client.catalog_search(body).await.map_err(client_err)?,
            ))
        },
    )
}
