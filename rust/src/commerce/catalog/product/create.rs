//! `gddy commerce catalog product create` — add a product to a store.

use std::future::Future;

use cli_engine::{CommandResult, CommandSpec, Result, RuntimeCommandSpec, Tier};
use serde_json::to_value;

use super::{
    fields::{parse_kind, parse_label, parse_status},
    output::ProductDetail,
    result::{ChangeResult, change_columns, changed, preview},
};
use crate::{
    commerce::{
        catalog::{
            client::CatalogClient, connect, operations::product_create::MutationCreateSkuGroupInput,
        },
        store::StoreIdArg,
    },
    scopes::COMMERCE_PRODUCT_WRITE,
};

#[derive(Debug, Clone, clap::Args)]
struct CreateArgs {
    #[command(flatten)]
    store: StoreIdArg,

    /// The product's name, as customers see it.
    #[arg(long, value_name = "LABEL", value_parser = parse_label)]
    label: String,

    /// What kind of product it is: physical or digital.
    #[arg(long = "type", value_name = "TYPE", value_parser = parse_kind)]
    kind: String,

    /// A description of the product.
    #[arg(long, value_name = "TEXT")]
    description: Option<String>,

    /// Whether the product is a draft or active (the API's own default applies
    /// when this is left off).
    #[arg(long, value_name = "STATUS", value_parser = parse_status)]
    status: Option<String>,

    /// A short handle for the product: lowercase letters, digits, `-` and `_`.
    /// The API makes one from the label when this is left off.
    #[arg(long, value_name = "HANDLE")]
    name: Option<String>,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<CreateArgs, _, _, _>(
        CommandSpec::from_args::<CreateArgs>("create", "Add a product to your store")
            .with_long(
                "Adds a product to your store. A product needs a label (its name \
                 as customers see it) and a type, physical or digital. Prices, \
                 variants and images are added separately.\n\
                 \n\
                 Examples:\n  \
                 gddy commerce catalog product create --label \"Classic Tee\" --type physical\n  \
                 gddy commerce catalog product create --label \"Guide\" --type digital --status active",
            )
            .with_system("commerce")
            .with_tier(Tier::Mutate)
            .mutates(true)
            .handles_dry_run(true)
            .with_output_schema::<ChangeResult>()
            .with_view(change_columns())
            .with_scopes(&[COMMERCE_PRODUCT_WRITE]),
        |ctx, args: CreateArgs| async move {
            create(&args, ctx.dry_run(), || {
                connect(&ctx, &args.store, &[COMMERCE_PRODUCT_WRITE])
            })
            .await
        },
    )
}

/// What `create` sends: only what the person gave, so the API's own defaults
/// apply to everything else.
fn input(args: &CreateArgs) -> MutationCreateSkuGroupInput {
    MutationCreateSkuGroupInput {
        label: args.label.clone(),
        type_: args.kind.clone(),
        description: args.description.clone(),
        status: args.status.clone(),
        name: args.name.clone(),
        ..MutationCreateSkuGroupInput::default()
    }
}

/// What `create` does, with the way it reaches the API passed in so a test can
/// see it is used once. Under `--dry-run` it is still used, since the store and
/// the sign-in are what the real run needs and a preview should fail where the
/// real run would, but nothing is created.
async fn create<F, Fut>(args: &CreateArgs, dry_run: bool, connect: F) -> Result<CommandResult>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<CatalogClient>>,
{
    let client = connect().await?;
    if dry_run {
        return Ok(preview(
            "dry-run: would create product",
            None,
            &[
                ("label", Some(&args.label)),
                ("type", Some(&args.kind)),
                ("description", args.description.as_deref()),
                ("status", args.status.as_deref()),
                ("name", args.name.as_deref()),
            ],
        ));
    }
    let product = ProductDetail::from(client.create_product(input(args)).await?);
    changed(to_value(product), args.store.store_id.as_deref())
}

#[cfg(test)]
mod tests;
