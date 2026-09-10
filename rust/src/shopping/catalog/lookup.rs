use cli_engine::{CommandResult, CommandSpec, RuntimeCommandSpec, Tier};

use crate::output_schema::output_schema;
use crate::shopping::SHOPPING_SCOPES;
use crate::shopping::common::{
    client_err, currency_code, make_client, merge_context_currency, read_json,
};

output_schema!(CatalogLookupOutput {
    "ucp": "object";
    "products": "[]object";
    "messages": "[]object";
});

#[derive(Debug, Clone, clap::Args)]
struct Args {
    /// Lookup request as raw JSON, including the `ids` array.
    #[arg(long, value_name = "JSON", required_unless_present = "file")]
    body: Option<String>,

    /// Path to a JSON lookup request. Takes precedence over --body.
    #[arg(long, value_name = "PATH")]
    file: Option<String>,

    /// Preferred ISO 4217 currency for returned catalog prices (for example, USD or GBP).
    #[arg(long, value_name = "CODE", value_parser = currency_code)]
    currency: Option<String>,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<Args, _, _, _>(
        CommandSpec::from_args::<Args>("lookup", "Resolve known Shopping catalog IDs")
            .with_long(
                "Submit a UCP catalog-lookup JSON object containing one or more `ids`. Unknown IDs \
                 are reported in the response messages rather than failing the whole request.",
            )
            .with_system("shopping")
            .with_tier(Tier::Read)
            .with_scopes(SHOPPING_SCOPES)
            .with_output_schema::<CatalogLookupOutput>(),
        |ctx, args: Args| async move {
            let mut body = read_json(args.body.as_deref(), args.file.as_deref(), "object")?;
            merge_context_currency(&mut body, args.currency.as_deref())?;
            let client = make_client(&ctx).await?;
            Ok(CommandResult::new(
                client.catalog_lookup(body).await.map_err(client_err)?,
            ))
        },
    )
}
