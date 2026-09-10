use cli_engine::{CommandResult, CommandSpec, RuntimeCommandSpec, Tier};
use serde_json::json;

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
    /// Product or variant ID to resolve. Repeat to resolve multiple IDs.
    #[arg(long, value_name = "ID", required_unless_present_any = ["body", "file"])]
    id: Vec<String>,

    /// Preferred ISO 4217 currency for returned catalog prices (for example, USD or GBP).
    #[arg(long, value_name = "CODE", value_parser = currency_code)]
    currency: Option<String>,

    /// Lookup request as raw JSON for advanced Shopping API filters and extensions.
    #[arg(long, value_name = "JSON")]
    body: Option<String>,

    /// Path to a JSON lookup request. Takes precedence over --body.
    #[arg(long, value_name = "PATH")]
    file: Option<String>,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<Args, _, _, _>(
        CommandSpec::from_args::<Args>("lookup", "Resolve known Shopping catalog IDs")
            .with_long(
                "Resolve one or more known product or variant IDs with repeatable --id. Unknown IDs \
                 are reported in the response messages rather than failing the whole request. Use \
                 --body or --file only for advanced Shopping API fields.",
            )
            .with_system("shopping")
            .with_tier(Tier::Read)
            .with_scopes(SHOPPING_SCOPES)
            .with_output_schema::<CatalogLookupOutput>(),
        |ctx, args: Args| async move {
            let mut body = if args.body.is_some() || args.file.is_some() {
                read_json(args.body.as_deref(), args.file.as_deref(), "object")?
            } else {
                json!({})
            };
            if !args.id.is_empty() {
                let object = body
                    .as_object_mut()
                    .expect("catalog lookup request is an object");
                if object.contains_key("ids") {
                    return Err(crate::error::GddyError::validation(
                        "--id conflicts with ids in the request body",
                    )
                    .into_cli_error());
                }
                object.insert("ids".to_owned(), json!(args.id));
            }
            merge_context_currency(&mut body, args.currency.as_deref())?;
            let client = make_client(&ctx).await?;
            Ok(CommandResult::new(
                client.catalog_lookup(body).await.map_err(client_err)?,
            ))
        },
    )
}
