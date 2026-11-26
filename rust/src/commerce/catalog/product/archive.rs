//! `gddy commerce catalog product archive` — take a product out of sale.

use std::future::Future;

use cli_engine::{CommandResult, CommandSpec, Result, RuntimeCommandSpec, Tier};
use serde_json::to_value;

use super::{
    get::{parse_product_id, product_not_found},
    output::ProductDetail,
    result::{ChangeResult, change_columns, changed, preview},
};
use crate::{
    commerce::{
        catalog::{client::CatalogClient, connect},
        store::StoreIdArg,
    },
    scopes::COMMERCE_PRODUCT_WRITE,
};

#[derive(Debug, Clone, clap::Args)]
struct ArchiveArgs {
    #[command(flatten)]
    store: StoreIdArg,

    /// The product to archive, as listed by `gddy commerce catalog product list`.
    #[arg(value_name = "PRODUCT_ID")]
    product_id: String,

    /// Archive the product's variants too. Without it only the product is
    /// archived, and its variants keep their status.
    #[arg(long)]
    with_variants: bool,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<ArchiveArgs, _, _, _>(
        CommandSpec::from_args::<ArchiveArgs>("archive", "Archive a product")
            .with_long(
                "Archives a product: it is no longer for sale, and nothing is \
                 deleted. By default only the product is archived and its variants \
                 keep their status; --with-variants archives them too. Find a \
                 product's id with `gddy commerce catalog product list`.\n\
                 \n\
                 Examples:\n  \
                 gddy commerce catalog product archive 6d1f0f4c-8a2e-4a52-9a5b-0f6a3c1d2e4b\n  \
                 gddy commerce catalog product archive 6d1f0f4c-8a2e-4a52-9a5b-0f6a3c1d2e4b --with-variants",
            )
            .with_system("commerce")
            .with_tier(Tier::Mutate)
            .mutates(true)
            .handles_dry_run(true)
            .with_output_schema::<ChangeResult>()
            .with_view(change_columns())
            .with_scopes(&[COMMERCE_PRODUCT_WRITE]),
        |ctx, args: ArchiveArgs| async move {
            archive(&args, &ctx.middleware.env, ctx.dry_run(), || {
                connect(&ctx, &args.store, &[COMMERCE_PRODUCT_WRITE])
            })
            .await
        },
    )
}

/// What `archive` does, with the way it reaches the API passed in so a test can
/// see that a bad id is refused before anything is requested. Under `--dry-run`
/// the product is read and nothing is archived.
async fn archive<F, Fut>(
    args: &ArchiveArgs,
    env: &str,
    dry_run: bool,
    connect: F,
) -> Result<CommandResult>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<CatalogClient>>,
{
    let store_flag = args.store.store_id.as_deref();
    let product_id = parse_product_id(&args.product_id, store_flag)?;
    let client = connect().await?;
    if dry_run {
        // The product has to be there for the real run to archive it.
        client
            .get_product(&product_id)
            .await?
            .ok_or_else(|| product_not_found(&product_id, env, store_flag))?;
        return Ok(preview(
            "dry-run: would archive product",
            Some(&product_id),
            &[(
                "variants",
                Some(if args.with_variants {
                    "archived with the product"
                } else {
                    "keep their status"
                }),
            )],
        ));
    }
    let product = client
        .archive_product(&product_id, args.with_variants)
        .await?
        .ok_or_else(|| product_not_found(&product_id, env, store_flag))?;
    changed(to_value(ProductDetail::from(product)), store_flag)
}

#[cfg(test)]
mod tests;
