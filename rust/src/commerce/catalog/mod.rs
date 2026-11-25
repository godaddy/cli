//! `gddy commerce catalog` — the merchant-facing product catalog, backed by the
//! Commerce Catalog GraphQL subgraph.

mod client;
mod operations;
mod product;

use cli_engine::{CommandContext, GroupSpec, Result, RuntimeGroupSpec};

use crate::commerce::store::{StoreIdArg, context::StoreIdInput, resolve_store_id};
use client::CatalogClient;

pub(super) fn group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(
        GroupSpec::new("catalog", "Manage your product catalog").with_long(
            "Manage the products in your catalog. Commands work on the store \
            chosen with `gddy commerce store use`, or the one given with \
            --store-id.",
        ),
    )
    .with_group(product::group())
}

/// A client for the store a catalog command should work on: `--store-id` if it
/// was given, else the selected store, authenticated for `scopes`.
async fn connect(
    ctx: &CommandContext,
    store: &StoreIdArg,
    scopes: &[&str],
) -> Result<CatalogClient> {
    let store_id = resolve_store_id(
        &ctx.middleware.env,
        store.store_id.as_deref(),
        StoreIdInput::Flag,
    )?;
    CatalogClient::connect(ctx, &store_id, scopes).await
}
