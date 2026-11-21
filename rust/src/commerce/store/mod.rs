//! Which store a `gddy commerce` command operates on.
//!
//! Every catalog and store call needs a store id, and repeating it on each
//! invocation is friction this CLI avoids elsewhere. [`resolve_store_id`] picks
//! one in a fixed order — the `--store-id` flag, then the `GDDY_STORE_ID`
//! environment variables, then the selection persisted for the active
//! environment (see [`context`]) — so a command can omit the flag once a store
//! has been selected.
//!
//! There is no way to list stores: the Businesses API that enumerated them was
//! withdrawn upstream. A store id is created when GoDaddy Payments is set up, and
//! today the only place to see it is the developer portal's "Your stores" page
//! ([`FIND_YOUR_STORE_ID_URL`]).

pub(crate) mod context;

use cli_engine::{CommandContext, Result};

use crate::error::GddyError;

use context::ResolvedStore;

/// Where to find a store id now that there is no in-CLI discovery. Surfaced from
/// the errors below and, later, from `store use`'s help.
pub(crate) const FIND_YOUR_STORE_ID_URL: &str =
    "https://developer.godaddy.com/docs/api-users/commerce#your-stores";

/// `--store-id`, shared by every command that operates on a store. Resolved with
/// [`resolve_store_id`] when omitted.
#[derive(Debug, Clone, clap::Args)]
pub(crate) struct StoreIdArg {
    /// Store to operate on. Defaults to the store selected with `gddy commerce
    /// store use`, or GDDY_STORE_ID / GDDY_STORE_ID_<ENV>.
    #[arg(long, value_name = "STORE_ID")]
    pub(crate) store_id: Option<String>,
}

/// The store id a command should operate on: `explicit` (the `--store-id`
/// flag), then the environment variables, then the persisted selection for the
/// active environment.
///
/// # Errors
///
/// A validation error if a supplied id is not a valid store id, a config error
/// if the persisted selection cannot be read, and a config error naming the ways
/// to supply one if nothing is selected.
pub(crate) fn resolve_store_id(ctx: &CommandContext, explicit: Option<&str>) -> Result<String> {
    require_store(context::lookup(&ctx.middleware.env, explicit)?).map(|store| store.store_id)
}

/// Turns "nothing selected" into the error that tells the user how to select one.
fn require_store(resolved: Option<ResolvedStore>) -> Result<ResolvedStore> {
    resolved.ok_or_else(|| {
        GddyError::config(
            "no store selected: pass --store-id, set GDDY_STORE_ID, or run \
             `gddy commerce store use <store-id>` to persist one for this environment",
        )
        .with_fix(format!(
            "Find your store id by signing in at {FIND_YOUR_STORE_ID_URL} (\"Your stores\"), \
             then pass it with --store-id or run: gddy commerce store use <store-id>"
        ))
        .into_cli_error()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use context::StoreIdSource;

    #[test]
    fn nothing_selected_names_every_way_to_select_a_store() {
        let error = require_store(None).expect_err("nothing selected is an error");
        let envelope = cli_engine::build_error_envelope(&error, "commerce");
        let message = envelope
            .error
            .as_ref()
            .map(|e| e.message.clone())
            .unwrap_or_default();
        for expected in ["--store-id", "GDDY_STORE_ID", "store use"] {
            assert!(message.contains(expected), "{expected} missing: {message}");
        }
        assert_eq!(
            envelope.error.as_ref().map(|e| e.code.as_str()),
            Some(crate::error::codes::CONFIG_ERROR)
        );
        assert!(
            envelope
                .fix
                .as_deref()
                .is_some_and(|fix| fix.contains(FIND_YOUR_STORE_ID_URL)),
            "the fix should say where to find a store id: {envelope:?}"
        );
    }

    #[test]
    fn a_selected_store_passes_through() {
        let resolved = ResolvedStore {
            store_id: "6d1f0f4c-8a2e-4a52-9a5b-0f6a3c1d2e4b".to_owned(),
            source: StoreIdSource::Flag,
            entry: None,
        };
        let store = require_store(Some(resolved)).expect("a selected store is returned");
        assert_eq!(store.store_id, "6d1f0f4c-8a2e-4a52-9a5b-0f6a3c1d2e4b");
    }
}
