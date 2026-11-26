//! `gddy commerce catalog product update` — change a product's details.

use std::future::Future;

use cli_engine::{CommandResult, CommandSpec, Result, RuntimeCommandSpec, Tier};
use serde_json::to_value;

use super::{
    fields::{parse_label, parse_status},
    get::{ProductResult, parse_product_id, product_not_found},
    output::ProductDetail,
    result::changed,
};
use crate::{
    commerce::{
        catalog::{
            client::CatalogClient, connect, operations::product_update::MutationUpdateSkuGroupInput,
        },
        store::StoreIdArg,
    },
    error::GddyError,
    scopes::COMMERCE_PRODUCT_WRITE,
};

#[derive(Debug, Clone, clap::Args)]
struct UpdateArgs {
    #[command(flatten)]
    store: StoreIdArg,

    /// The product to change, as listed by `gddy commerce catalog product list`.
    #[arg(value_name = "PRODUCT_ID")]
    product_id: String,

    /// A new name, as customers see it.
    #[arg(long, value_name = "LABEL", value_parser = parse_label)]
    label: Option<String>,

    /// A new description.
    #[arg(long, value_name = "TEXT")]
    description: Option<String>,

    /// Set the product to draft or active.
    #[arg(long, value_name = "STATUS", value_parser = parse_status)]
    status: Option<String>,

    /// A new short handle: lowercase letters, digits, `-` and `_`.
    #[arg(long, value_name = "HANDLE")]
    name: Option<String>,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<UpdateArgs, _, _, _>(
        CommandSpec::from_args::<UpdateArgs>("update", "Change a product's details")
            .with_long(
                "Changes the details you name and leaves the rest as they are. At \
                 least one of --label, --description, --status or --name is \
                 required. Find a product's id with `gddy commerce catalog product \
                 list`. To archive a product, use `product archive`.\n\
                 \n\
                 Examples:\n  \
                 gddy commerce catalog product update 6d1f0f4c-8a2e-4a52-9a5b-0f6a3c1d2e4b --label \"Classic T-Shirt\"\n  \
                 gddy commerce catalog product update 6d1f0f4c-8a2e-4a52-9a5b-0f6a3c1d2e4b --status active",
            )
            .with_system("commerce")
            .with_tier(Tier::Mutate)
            .mutates(true)
            .with_output_schema::<ProductResult>()
            .with_view(super::get::view_columns())
            .with_scopes(&[COMMERCE_PRODUCT_WRITE]),
        |ctx, args: UpdateArgs| async move {
            update(&args, &ctx.middleware.env, || {
                connect(&ctx, &args.store, &[COMMERCE_PRODUCT_WRITE])
            })
            .await
        },
    )
}

/// What `update` sends: only what the person gave, so everything else is left
/// as it is. `None` when nothing was given, since there is then nothing to change.
fn input(args: &UpdateArgs) -> Option<MutationUpdateSkuGroupInput> {
    let input = MutationUpdateSkuGroupInput {
        label: args.label.clone(),
        description: args.description.clone(),
        status: args.status.clone(),
        name: args.name.clone(),
        ..MutationUpdateSkuGroupInput::default()
    };
    let nothing = args.label.is_none()
        && args.description.is_none()
        && args.status.is_none()
        && args.name.is_none();
    (!nothing).then_some(input)
}

/// What `update` does, with the way it reaches the API passed in so a test can
/// see that a bad id or an empty change is refused before anything is requested.
async fn update<F, Fut>(args: &UpdateArgs, env: &str, connect: F) -> Result<CommandResult>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<CatalogClient>>,
{
    let store_flag = args.store.store_id.as_deref();
    let product_id = parse_product_id(&args.product_id, store_flag)?;
    let input = input(args).ok_or_else(|| {
        GddyError::validation("nothing to change: no new details were given")
            .with_fix(
                "Give at least one of --label, --description, --status or --name, \
                 for example: gddy commerce catalog product update <product-id> --label \"New name\"",
            )
            .with_system("commerce")
            .into_cli_error()
    })?;
    let client = connect().await?;
    let product = client
        .update_product(&product_id, input)
        .await?
        .ok_or_else(|| product_not_found(&product_id, env, store_flag))?;
    changed(to_value(ProductDetail::from(product)), store_flag)
}

#[cfg(test)]
mod tests;
