//! `gddy commerce catalog` — the merchant-facing product catalog, backed by the
//! Commerce Catalog GraphQL subgraph.

mod client;
mod operations;
mod product;

use cli_engine::{CommandContext, GroupSpec, NextAction, Result, RuntimeGroupSpec};

use crate::{
    commerce::store::{StoreIdArg, context::StoreIdInput, resolve_store_id},
    next_action::{next_action, required_value},
};
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

/// A follow-up command for a catalog command to suggest. When the person named
/// the store with `--store-id`, the follow-up names it too, with the id kept in
/// a structured parameter: left out, it would run against the saved store, or fail
/// if there is none. Without `--store-id` the follow-up resolves the store the
/// same way the command did, from the environment or the saved selection, so it
/// has no flag to repeat.
fn follow_up(command: &str, description: &str, store_flag: Option<&str>) -> NextAction {
    match store_flag {
        Some(store_id) => next_action(format!("{command} --store-id <store-id>"), description)
            .with_param("store-id", required_value(store_id)),
        None => next_action(command, description),
    }
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
