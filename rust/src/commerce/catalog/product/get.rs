//! `gddy commerce catalog product get` — one product's details.

use std::future::Future;

use cli_engine::{
    CliCoreError, CommandResult, CommandSpec, Result, RuntimeCommandSpec, TableColumn, Tier,
};
use serde_json::{Value, to_value};
use uuid::Uuid;

use super::output::ProductDetail;
use crate::{
    commerce::{
        catalog::{client::CatalogClient, connect, follow_up},
        store::StoreIdArg,
    },
    error::GddyError,
    output_schema::output_schema,
    scopes::COMMERCE_PRODUCT_READ,
};

// Also what `create`, `update` and `archive` print: the product as it now is.
output_schema!(ProductResult {
    "id": "string";
    "name": "string";
    "label": "string";
    "status": "string";
    "type": "string";
    "description": "string", optional;
    "variantCount": "number", optional;
    "priceMin": "number", optional;
    "priceMax": "number", optional;
    "compareAtPriceMin": "number", optional;
    "compareAtPriceMax": "number", optional;
    "htmlDescription": "string", optional;
    "createdAt": "string";
    "updatedAt": "string";
    "activatedAt": "string", optional;
    "archivedAt": "string", optional;
});

/// The fields `get` shows people, labelled in merchant terms.
pub(super) fn view_columns() -> Vec<TableColumn> {
    vec![
        TableColumn::new("id", "ID"),
        TableColumn::new("label", "Name"),
        TableColumn::new("name", "Handle"),
        TableColumn::new("status", "Status"),
        TableColumn::new("type", "Type"),
        TableColumn::new("description", "Description"),
        TableColumn::new("variantCount", "Variants"),
        TableColumn::new("priceMin", "Price from"),
        TableColumn::new("priceMax", "Price to"),
        TableColumn::new("compareAtPriceMin", "Compare-at from"),
        TableColumn::new("compareAtPriceMax", "Compare-at to"),
        TableColumn::new("createdAt", "Created"),
        TableColumn::new("updatedAt", "Updated"),
        TableColumn::new("activatedAt", "Activated"),
        TableColumn::new("archivedAt", "Archived"),
    ]
}

#[derive(Debug, Clone, clap::Args)]
struct GetArgs {
    #[command(flatten)]
    store: StoreIdArg,

    /// The product to show, as listed by `gddy commerce catalog product list`.
    #[arg(value_name = "PRODUCT_ID")]
    product_id: String,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<GetArgs, _, _, _>(
        CommandSpec::from_args::<GetArgs>("get", "Show a product's details")
            .with_long(
                "Shows a product's name, status, description, lowest and highest \
                price (in your store's currency) and dates. Find a product's id \
                with `gddy commerce catalog product list`.\n\
                \n\
                Examples:\n  \
                gddy commerce catalog product get 6d1f0f4c-8a2e-4a52-9a5b-0f6a3c1d2e4b",
            )
            .with_system("commerce")
            .with_tier(Tier::Read)
            .with_output_schema::<ProductResult>()
            .with_view(view_columns())
            .with_scopes(&[COMMERCE_PRODUCT_READ]),
        |ctx, args: GetArgs| async move {
            get(&args, &ctx.middleware.env, || {
                connect(&ctx, &args.store, &[COMMERCE_PRODUCT_READ])
            })
            .await
        },
    )
}

/// What `get` does, with the way it reaches the API passed in so a test can see
/// whether it was used: the id is checked first, and only a valid one connects.
async fn get<F, Fut>(args: &GetArgs, env: &str, connect: F) -> Result<CommandResult>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<CatalogClient>>,
{
    let store_flag = args.store.store_id.as_deref();
    let product_id = parse_product_id(&args.product_id, store_flag)?;
    let client = connect().await?;
    show(&client, &product_id, env, args.store.store_id.as_deref()).await
}

/// The command that lists the store's products, for a hint to send the person to,
/// naming the store when `--store-id` chose it: left out, the hint would list the
/// selected store, or fail if there is none. The id is repeated only in its
/// canonical form and only if it is one, so nothing else a person typed is ever
/// put into a suggested command.
pub(super) fn list_command(store_flag: Option<&str>) -> String {
    let store = store_flag.and_then(|id| Uuid::parse_str(id.trim()).ok());
    match store {
        Some(id) => format!(
            "gddy commerce catalog product list --store-id {}",
            id.hyphenated()
        ),
        None => "gddy commerce catalog product list".to_owned(),
    }
}

/// The product id in the form the API takes: a UUID, hyphenated. Checked here,
/// before anything is sent, so a mistyped id is named as one instead of coming
/// back as a refused request.
pub(super) fn parse_product_id(value: &str, store_flag: Option<&str>) -> Result<String> {
    Uuid::parse_str(value.trim())
        .map(|id| id.hyphenated().to_string())
        .map_err(|_| {
            GddyError::validation(format!("{value:?} is not a product id"))
                .with_fix(format!(
                    "A product id is a UUID such as 6d1f0f4c-8a2e-4a52-9a5b-0f6a3c1d2e4b. \
                     Find it with: {}",
                    list_command(store_flag)
                ))
                .with_system("commerce")
                .into_cli_error()
        })
}

/// The error for a product the store does not have: it names the product and
/// the environment, and says how to find real ones.
pub(super) fn product_not_found(
    product_id: &str,
    env: &str,
    store_flag: Option<&str>,
) -> CliCoreError {
    GddyError::not_found(format!(
        "no product {product_id:?} was found in this store in the {env} environment"
    ))
    .with_fix(format!(
        "List the store's products with: {}",
        list_command(store_flag)
    ))
    .with_system("commerce")
    .into_cli_error()
}

/// Reads one product and shows it. A product the store does not have is a
/// not-found error that names it, not an empty result.
async fn show(
    client: &CatalogClient,
    product_id: &str,
    env: &str,
    store_flag: Option<&str>,
) -> Result<CommandResult> {
    let product = client
        .get_product(product_id)
        .await?
        .ok_or_else(|| product_not_found(product_id, env, store_flag))?;
    let data: Value = to_value(ProductDetail::from(product)).map_err(|_| {
        GddyError::unexpected("the product could not be shown")
            .with_system("commerce")
            .into_cli_error()
    })?;
    Ok(CommandResult::new(data).with_next_actions(vec![follow_up(
        "commerce catalog product list",
        "List all products",
        store_flag,
    )]))
}

#[cfg(test)]
mod tests;
