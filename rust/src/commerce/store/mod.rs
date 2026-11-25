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
//! today the only place to see it is the developer portal's "Your stores" page,
//! which [`find_your_store_id_hint`] points to for the active environment.

mod commands;
pub(crate) mod context;

use cli_engine::{CommandContext, Result};

use crate::{environments, error::GddyError};

pub(crate) use commands::group;
use context::ResolvedStore;

/// Path of the "Your stores" section on a developer portal.
const YOUR_STORES_PATH: &str = "/docs/api-users/commerce#your-stores";

/// A sentence telling the user where to find a store id in `env`, now that there
/// is no in-CLI discovery. The portal is the environment's own, so a store id for
/// `ote` is looked up on the OTE portal, and `GDDY_DEVELOPER_URL` is honored.
///
/// The link is built with a URL parser, not by appending text: an override may
/// carry a query string or a fragment, and appending the docs path to one of
/// those would produce a link that goes nowhere. If the environment cannot be
/// resolved, or its portal is not a usable base, the sentence names the page
/// without a link, and says why when it is the configuration that is broken,
/// rather than guessing at a portal that might be the wrong one.
pub(crate) fn find_your_store_id_hint(env: &str) -> String {
    const PAGE: &str = "the \"Your stores\" page of the GoDaddy developer portal";
    let resolved = match environments::resolve(env) {
        Ok(resolved) => resolved,
        Err(error) => {
            return format!(
                "Find your store id on {PAGE}. (The {env} environment's configuration could not \
                 be read, so no link is shown: {error})"
            );
        }
    };
    match your_stores_url(&resolved.developer_url) {
        Some(link) => format!("Find your store id by signing in at {link} (\"Your stores\")."),
        None => format!("Find your store id on {PAGE}."),
    }
}

/// The "Your stores" page under the developer portal at `portal`, keeping any
/// path prefix the portal is served under and dropping its query string and
/// fragment. `None` if `portal` is not a base URL a path can be added to.
fn your_stores_url(portal: &str) -> Option<String> {
    let mut url = url::Url::parse(portal).ok()?;
    if url.cannot_be_a_base() {
        return None;
    }
    let (path, fragment) = YOUR_STORES_PATH.split_once('#')?;
    let mut base = url.path().trim_end_matches('/').to_owned();
    base.push_str(path);
    url.set_path(&base);
    url.set_query(None);
    url.set_fragment(Some(fragment));
    Some(url.into())
}

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
    let env = &ctx.middleware.env;
    require_store(context::lookup(env, explicit)?, env).map(|store| store.store_id)
}

/// Turns "nothing selected" into the error that tells the user how to select one.
fn require_store(resolved: Option<ResolvedStore>, env: &str) -> Result<ResolvedStore> {
    resolved.ok_or_else(|| {
        GddyError::config(
            "no store selected: pass --store-id, set GDDY_STORE_ID, or run \
             `gddy commerce store use <store-id>` to persist one for this environment",
        )
        .with_fix(format!(
            "{} Then pass it with --store-id or run: gddy commerce store use <store-id>",
            find_your_store_id_hint(env)
        ))
        .into_cli_error()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::environments::test_support::{ENV_LOCK, EnvGuard};
    use context::StoreIdSource;

    fn fix_of(error: &cli_engine::CliCoreError) -> String {
        cli_engine::build_error_envelope(error, "commerce")
            .fix
            .unwrap_or_default()
    }

    #[test]
    fn nothing_selected_names_every_way_to_select_a_store() {
        let _lock = ENV_LOCK.blocking_lock();
        let error = require_store(None, "ote").expect_err("nothing selected is an error");
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
                .is_some_and(|fix| fix.contains("Your stores")),
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
        let store = require_store(Some(resolved), "ote").expect("a selected store is returned");
        assert_eq!(store.store_id, "6d1f0f4c-8a2e-4a52-9a5b-0f6a3c1d2e4b");
    }

    /// The portal to send a user to depends on which environment's store they
    /// are looking for: an OTE store id is not on the production portal.
    #[test]
    fn the_hint_points_at_the_portal_of_the_environment_asked_about() {
        let _lock = ENV_LOCK.blocking_lock();
        let _unset = EnvGuard::unset("GDDY_DEVELOPER_URL");
        let prod = find_your_store_id_hint("prod");
        let ote = find_your_store_id_hint("ote");
        assert!(
            prod.contains("https://developer.godaddy.com/docs/api-users/commerce#your-stores"),
            "{prod}"
        );
        assert!(
            ote.contains("https://developer.ote-godaddy.com/docs/api-users/commerce#your-stores"),
            "{ote}"
        );
        assert!(!ote.contains("https://developer.godaddy.com"), "{ote}");
    }

    #[test]
    fn the_hint_honors_a_developer_url_override_without_a_doubled_slash() {
        let _lock = ENV_LOCK.blocking_lock();
        let _override = EnvGuard::set("GDDY_DEVELOPER_URL", "https://developer.override.test/");
        let hint = find_your_store_id_hint("ote");
        assert!(
            hint.contains("https://developer.override.test/docs/api-users/commerce#your-stores"),
            "{hint}"
        );
        assert!(!hint.contains("test//docs"), "{hint}");
    }

    #[test]
    fn the_hint_for_an_unknown_environment_names_the_page_without_guessing_a_portal() {
        let _lock = ENV_LOCK.blocking_lock();
        let hint = find_your_store_id_hint("no-such-environment");
        assert!(hint.contains("Your stores"), "{hint}");
        assert!(
            !hint.contains("https://"),
            "no link should be guessed: {hint}"
        );
    }

    /// Both errors that carry the hint — nothing selected, and an id that is not
    /// one — must use the active environment's portal.
    #[test]
    fn both_errors_use_the_active_environments_portal() {
        let _lock = ENV_LOCK.blocking_lock();
        let _unset = EnvGuard::unset("GDDY_DEVELOPER_URL");

        let missing = require_store(None, "ote").expect_err("nothing selected");
        assert!(
            fix_of(&missing).contains("developer.ote-godaddy.com"),
            "{}",
            fix_of(&missing)
        );

        let invalid = context::lookup("ote", Some("nope")).expect_err("not a store id");
        assert!(
            fix_of(&invalid).contains("developer.ote-godaddy.com"),
            "{}",
            fix_of(&invalid)
        );
    }

    /// `clean_url` accepts an override with a query string or a fragment, so the
    /// hint must not splice the docs path onto the end of one of those.
    #[test]
    fn an_override_with_a_query_or_fragment_still_yields_a_well_formed_link() {
        let _lock = ENV_LOCK.blocking_lock();
        for override_url in [
            "https://dev.test/?x=1",
            "https://dev.test/#top",
            "https://dev.test?x=1#top",
        ] {
            let _override = EnvGuard::set("GDDY_DEVELOPER_URL", override_url);
            let hint = find_your_store_id_hint("ote");
            let link = hint
                .split_whitespace()
                .find(|word| word.starts_with("https://"));
            assert!(link.is_some(), "no link for {override_url}: {hint}");
            let url = url::Url::parse(link.expect("checked above")).expect("the link parses");
            assert_eq!(url.host_str(), Some("dev.test"), "{override_url}: {hint}");
            assert_eq!(
                url.path(),
                "/docs/api-users/commerce",
                "{override_url}: {hint}"
            );
            assert_eq!(url.query(), None, "{override_url}: {hint}");
            assert_eq!(
                url.fragment(),
                Some("your-stores"),
                "{override_url}: {hint}"
            );
        }
    }

    #[test]
    fn a_path_prefix_on_the_override_is_kept() {
        let _lock = ENV_LOCK.blocking_lock();
        let _override = EnvGuard::set("GDDY_DEVELOPER_URL", "https://dev.test/portal/");
        let hint = find_your_store_id_hint("ote");
        assert!(
            hint.contains("https://dev.test/portal/docs/api-users/commerce#your-stores"),
            "{hint}"
        );
    }

    /// An environment whose configuration is broken still gets a hint, but the
    /// hint must say what is wrong instead of hiding it behind a generic sentence.
    #[test]
    fn an_environment_that_cannot_be_resolved_says_why_in_the_hint() {
        let _lock = ENV_LOCK.blocking_lock();
        let _override = EnvGuard::set("GDDY_DEVELOPER_URL", "not a url");
        let hint = find_your_store_id_hint("ote");
        assert!(hint.contains("Your stores"), "{hint}");
        assert!(hint.contains("not a url"), "the cause is hidden: {hint}");
        assert!(
            !hint.contains("https://"),
            "no link should be guessed: {hint}"
        );
    }
}
