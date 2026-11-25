//! `gddy commerce catalog product get` — one product's details.

use cli_engine::{CommandResult, CommandSpec, Result, RuntimeCommandSpec, TableColumn, Tier};
use serde_json::{Value, to_value};
use uuid::Uuid;

use super::output::ProductDetail;
use crate::{
    commerce::{
        catalog::{client::CatalogClient, connect},
        store::StoreIdArg,
    },
    error::GddyError,
    next_action::next_action,
    output_schema::output_schema,
    scopes::COMMERCE_PRODUCT_READ,
};

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
fn view_columns() -> Vec<TableColumn> {
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
            let product_id = parse_product_id(&args.product_id)?;
            let client = connect(&ctx, &args.store, &[COMMERCE_PRODUCT_READ]).await?;
            show(&client, &product_id, &ctx.middleware.env).await
        },
    )
}

/// The product id in the form the API takes: a UUID, hyphenated. Checked here,
/// before anything is sent, so a mistyped id is named as one instead of coming
/// back as a refused request.
fn parse_product_id(value: &str) -> Result<String> {
    Uuid::parse_str(value.trim())
        .map(|id| id.hyphenated().to_string())
        .map_err(|_| {
            GddyError::validation(format!("{value:?} is not a product id"))
                .with_fix(
                    "A product id is a UUID such as 6d1f0f4c-8a2e-4a52-9a5b-0f6a3c1d2e4b. \
                     Find it with: gddy commerce catalog product list",
                )
                .with_system("commerce")
                .into_cli_error()
        })
}

/// Reads one product and shows it. A product the store does not have is a
/// not-found error that names it, not an empty result.
async fn show(client: &CatalogClient, product_id: &str, env: &str) -> Result<CommandResult> {
    let product = client.get_product(product_id).await?.ok_or_else(|| {
        GddyError::not_found(format!(
            "no product {product_id:?} was found in this store in the {env} environment"
        ))
        .with_fix("List the store's products with: gddy commerce catalog product list")
        .with_system("commerce")
        .into_cli_error()
    })?;
    let data: Value = to_value(ProductDetail::from(product)).map_err(|_| {
        GddyError::unexpected("the product could not be shown")
            .with_system("commerce")
            .into_cli_error()
    })?;
    Ok(CommandResult::new(data).with_next_actions(vec![next_action(
        "commerce catalog product list",
        "List all products",
    )]))
}

#[cfg(test)]
mod tests;
