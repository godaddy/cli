//! `gddy commerce store get | use | current | clear`.
//!
//! `get` and `use` read the store from the Stores API; `current` and `clear` only
//! look at the selection saved on this machine, so they work without signing in.

use cli_engine::{
    CommandResult, CommandSpec, GroupSpec, Result, RuntimeCommandSpec, RuntimeGroupSpec,
    TableColumn, Tier,
};
use commerce_client::{Client, types::V1Store};
use serde_json::{Value, json};

use super::{
    context::{self, StoreEntry, StoreIdInput},
    resolve_store_id,
};
use crate::{
    commerce::client::{fetch_store, make_client},
    output_schema::output_schema,
    scopes::COMMERCE_STORE_READ,
};

const SYSTEM: &str = "commerce";

output_schema!(StoreResult {
    "storeId": "string";
    "businessId": "string", optional;
    "displayName": "string", optional;
    "status": "string", optional;
    "emailAddress": "string", optional;
    "currency": "string", optional;
    "createdAt": "string", optional;
    "updatedAt": "string", optional;
});

output_schema!(UseResult {
    "storeId": "string";
    "businessId": "string", optional;
    "displayName": "string", optional;
    "status": "string", optional;
    "environment": "string";
    "action": "string";
});

output_schema!(CurrentResult {
    "selected": "bool";
    "storeId": "string", optional;
    "businessId": "string", optional;
    "displayName": "string", optional;
    "environment": "string";
    "source": "string", optional;
});

output_schema!(ClearResult {
    "environment": "string";
    "storeId": "string", optional;
    "action": "string";
});

pub(crate) fn group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(GroupSpec::new(
        "store",
        "Choose and inspect the store the commerce commands work on",
    ))
    .with_command(get_command())
    .with_command(use_command())
    .with_command(current_command())
    .with_command(clear_command())
}

#[derive(Debug, Clone, clap::Args)]
struct GetArgs {
    /// Store to show. Defaults to the store chosen with `gddy commerce store use`,
    /// or GDDY_STORE_ID / GDDY_STORE_ID_<ENV>.
    #[arg(value_name = "STORE_ID")]
    store_id: Option<String>,
}

#[derive(Debug, Clone, clap::Args)]
struct UseArgs {
    /// Store to work on from now on, for example
    /// 6d1f0f4c-8a2e-4a52-9a5b-0f6a3c1d2e4b.
    #[arg(value_name = "STORE_ID")]
    store_id: String,
}

#[derive(Debug, Clone, clap::Args)]
struct NoArgs {}

fn store_columns() -> Vec<TableColumn> {
    vec![
        TableColumn::new("storeId", "Store ID"),
        TableColumn::new("displayName", "Name"),
        TableColumn::new("status", "Status"),
        TableColumn::new("businessId", "Business ID"),
        TableColumn::new("emailAddress", "Email"),
        TableColumn::new("currency", "Currency"),
        TableColumn::new("createdAt", "Created"),
        TableColumn::new("updatedAt", "Updated"),
    ]
}

/// The fields of `store` worth showing. `store_id` is the id that was asked for,
/// not the one in the response, which the API may leave out.
fn store_summary(store_id: &str, store: &V1Store) -> Value {
    json!({
        "storeId": store_id,
        "businessId": store.business_id,
        "displayName": store.display_name,
        "status": store.status.map(|status| status.to_string()),
        "emailAddress": store.email_address,
        "currency": store.currency.as_deref(),
        "createdAt": store.created_at,
        "updatedAt": store.updated_at,
    })
}

/// The store `get` shows: the id it was given, else the selected store. The id is
/// a positional argument, so errors about it say so.
fn get_target(env: &str, store_id: Option<&str>) -> Result<String> {
    resolve_store_id(env, store_id, StoreIdInput::Argument)
}

/// The store `use` selects, validated and canonicalized before anything is
/// fetched or saved. The id is a positional argument, so errors about it say so.
fn use_target(env: &str, store_id: &str) -> Result<String> {
    resolve_store_id(env, Some(store_id), StoreIdInput::Argument)
}

/// Reads the store and shows it.
async fn show(client: &Client, store_id: &str, env: &str) -> Result<CommandResult> {
    let store = fetch_store(client, store_id, env).await?;
    Ok(CommandResult::new(store_summary(store_id, &store)))
}

/// Reads the store, and only if that succeeds saves it as the selection for
/// `env`. Under `--dry-run` everything the real run needs is still checked, the
/// read of the store and that the selection file can be saved to, so a preview
/// fails where the real run would, but nothing is saved.
async fn select(
    client: &Client,
    store_id: &str,
    env: &str,
    dry_run: bool,
) -> Result<CommandResult> {
    let store = fetch_store(client, store_id, env).await?;
    if dry_run {
        context::check_can_save()?;
    } else {
        context::save_store(
            env,
            StoreEntry {
                store_id: store_id.to_owned(),
                business_id: store.business_id.clone(),
                display_name: store.display_name.clone(),
            },
        )?;
    }
    let result = CommandResult::new(json!({
        "storeId": store_id,
        "businessId": store.business_id,
        "displayName": store.display_name,
        "status": store.status.map(|status| status.to_string()),
        "environment": env,
        "action": if dry_run { "would select" } else { "selected" },
    }));
    Ok(if dry_run {
        result.with_dry_run()
    } else {
        result
    })
}

fn get_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<GetArgs, _, _, _>(
        CommandSpec::from_args::<GetArgs>("get", "Show a store's details")
            .with_long(
                "Shows a store's name, status and contact details. With no store \
                id it shows the store you chose with `gddy commerce store use`.\n\
                 \n\
                 Examples:\n  \
                 gddy commerce store get\n  \
                 gddy commerce store get 6d1f0f4c-8a2e-4a52-9a5b-0f6a3c1d2e4b",
            )
            .with_system(SYSTEM)
            .with_tier(Tier::Read)
            .with_default_fields("storeId,displayName,status,businessId,emailAddress,currency")
            .with_output_schema::<StoreResult>()
            .with_view(store_columns())
            .with_scopes(&[COMMERCE_STORE_READ]),
        |ctx, args: GetArgs| async move {
            let store_id = get_target(&ctx.middleware.env, args.store_id.as_deref())?;
            let client = make_client(&ctx, &[COMMERCE_STORE_READ]).await?;
            show(&client, &store_id, &ctx.middleware.env).await
        },
    )
}

fn use_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<UseArgs, _, _, _>(
        CommandSpec::from_args::<UseArgs>("use", "Choose the store the commerce commands work on")
            .with_long(
                "Checks that the store exists and that you can read it, then \
                remembers it so other commerce commands need no store id.\n\
                 \n\
                 There is no way to list your stores from the command line. Your \
                 store id is on the \"Your stores\" page of the GoDaddy \
                 developer portal.\n\
                 \n\
                 Examples:\n  \
                 gddy commerce store use 6d1f0f4c-8a2e-4a52-9a5b-0f6a3c1d2e4b",
            )
            .with_system(SYSTEM)
            .with_tier(Tier::Mutate)
            .handles_dry_run(true)
            .with_output_schema::<UseResult>()
            .with_scopes(&[COMMERCE_STORE_READ]),
        |ctx, args: UseArgs| async move {
            let env = ctx.middleware.env.clone();
            // Validates and canonicalizes the id before anything is fetched or saved.
            let store_id = use_target(&env, &args.store_id)?;
            let client = make_client(&ctx, &[COMMERCE_STORE_READ]).await?;
            select(&client, &store_id, &env, ctx.dry_run()).await
        },
    )
}

fn current_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<NoArgs, _, _, _>(
        CommandSpec::from_args::<NoArgs>("current", "Show the store the commerce commands use")
            .with_long(
                "Shows the currently selected store for commerce commands. \
                Choose one with `gddy commerce store use`.",
            )
            .with_system(SYSTEM)
            .with_tier(Tier::Read)
            .with_output_schema::<CurrentResult>()
            .no_auth(true),
        |ctx, _args: NoArgs| async move {
            let env = ctx.middleware.env.clone();
            let selected = context::lookup(&env, None, StoreIdInput::Flag)?;
            Ok(CommandResult::new(match selected {
                Some(store) => json!({
                    "selected": true,
                    "storeId": store.store_id,
                    "businessId": store.entry.as_ref().and_then(|entry| entry.business_id.clone()),
                    "displayName": store.entry.as_ref().and_then(|entry| entry.display_name.clone()),
                    "environment": env,
                    "source": store.source.as_str(),
                }),
                None => json!({
                    "selected": false,
                    "environment": env,
                }),
            }))
        },
    )
}

fn clear_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<NoArgs, _, _, _>(
        CommandSpec::from_args::<NoArgs>("clear", "Forget the chosen store for this environment")
            .with_long("Removes the store chosen with `gddy commerce store use`.")
            .with_system(SYSTEM)
            .with_tier(Tier::Mutate)
            .handles_dry_run(true)
            .with_output_schema::<ClearResult>()
            .no_auth(true),
        |ctx, _args: NoArgs| async move {
            let env = ctx.middleware.env.clone();
            let dry_run = ctx.dry_run();
            // A preview only looks. A real run reports the entry the locked clear
            // removed, not an earlier read, which another process may have
            // changed in between.
            let (removed, action) = if dry_run {
                let saved = context::saved_store(&env)?;
                let action = if saved.is_some() {
                    "would clear"
                } else {
                    "nothing to clear"
                };
                (saved, action)
            } else {
                let removed = context::clear_store(&env)?;
                let action = if removed.is_some() {
                    "cleared"
                } else {
                    "nothing to clear"
                };
                (removed, action)
            };
            let result = CommandResult::new(json!({
                "environment": env,
                "storeId": removed.map(|entry| entry.store_id),
                "action": action,
            }));
            Ok(if dry_run {
                result.with_dry_run()
            } else {
                result
            })
        },
    )
}

#[cfg(test)]
mod tests;
