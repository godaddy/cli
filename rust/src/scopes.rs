//! Central registry of every OAuth scope the `gddy` CLI requests.
//!
//! # Why this module exists
//!
//! A command declares the scopes it needs with
//! [`cli_engine::CommandSpec::with_scopes`], and
//! cli-engine's OAuth step-up mints a token carrying them. But a scope the CLI
//! *requests* is only grantable if the CLI's **OAuth client** is *registered* for
//! it — otherwise the authorization server refuses to mint a token carrying the
//! scope and the command fails at auth, with nothing in this codebase to catch
//! it. It is easy to add `.with_scopes(&["some.new:scope"])` to a command by
//! copying a neighbour and never realize the scope is unobtainable.
//!
//! So: **every scope the CLI uses is declared here, once**, and commands draw
//! from these constants rather than spelling out string literals. The [`ALL`]
//! slice is derived from the same declarations, so it is always the complete,
//! authoritative list of scopes the OAuth client must be registered for — a
//! single place to diff against the client's configuration.
//!
//! # Adding a scope (READ THIS)
//!
//! 1. Add a constant to the [`declare_scopes!`] block below. It is automatically
//!    included in [`ALL`] — you cannot add a scope constant without registering
//!    it in the list.
//! 2. Reference the new constant from the command via `.with_scopes(&[scopes::…])`.
//! 3. **Register the same scope on the CLI's OAuth client**, or it will be
//!    ungrantable at runtime.
//!
//! This registry covers the scopes the CLI requests for *itself* (login defaults
//! plus per-command step-up). It intentionally does NOT cover scopes that are
//! user data rather than the CLI's own grants — e.g. the `authorizationScopes`
//! assigned to a third-party app created via `gddy application create`, or the
//! ad-hoc scopes `gddy api call --scope …` derives from the API catalog at
//! runtime.

/// Declares each scope constant and derives [`ALL`] from the same list, so a
/// scope cannot exist without being part of the authoritative registry.
macro_rules! declare_scopes {
    ($( $(#[$doc:meta])* $name:ident => $value:literal ),+ $(,)?) => {
        $( $(#[$doc])* pub const $name: &str = $value; )+

        /// Every `resource:action` permission scope the CLI may request. Auto-derived
        /// from the constants declared in [`declare_scopes!`] — keep the client's
        /// registration in sync with this list.
        ///
        /// NOT the complete set of scopes requiring OAuth client registration:
        /// [`OFFLINE_ACCESS`] is a directive scope (not a `resource:action` permission)
        /// and is deliberately excluded, but still must be registered on the client
        /// server-side. Diff the client's configuration against `ALL` *plus*
        /// [`OFFLINE_ACCESS`], not `ALL` alone.
        ///
        /// Not referenced by production code (the individual constants are what
        /// commands use); it exists as the authoritative registry to diff against
        /// the OAuth client's configuration and is exercised by the module tests.
        #[allow(dead_code)]
        pub const ALL: &[&str] = &[ $($name),+ ];
    };
}

/// OIDC directive scope requesting a `refresh_token` alongside the access
/// token. Requested at login by default (see
/// [`crate::environments::DEFAULT_OAUTH_SCOPES`]).
///
/// Deliberately declared outside [`declare_scopes!`]/[`ALL`]: unlike the
/// scopes below, it isn't a `resource:action` permission grant, so it fails
/// the `resource:action` shape the module tests enforce for `ALL`. It still
/// must be registered on the CLI's OAuth client server-side, or the
/// authorization server will refuse or silently drop it just like any other
/// unregistered scope.
pub const OFFLINE_ACCESS: &str = "offline_access";

// DON'T FORGET! If you add a scope here, you must also register it on the CLI's OAuth client.
declare_scopes! {
    /// Read the caller's registered applications. Requested at login by default
    /// (see [`crate::environments::DEFAULT_OAUTH_SCOPES`]).
    APP_REGISTRY_READ => "apps.app-registry:read",
    /// Create/update/archive the caller's registered applications. NOT requested
    /// at login by default (it's a rare operation for most customers); the
    /// app-registry mutation commands (`application init/update/enable/disable/
    /// archive/release/deploy`) declare it via `with_scopes` so cli-engine
    /// requests it on demand (OAuth step-up).
    APP_REGISTRY_WRITE => "apps.app-registry:write",

    /// Read domains, availability, suggestions, quotes, and DNS records.
    /// (`domain list/get/available/suggest/agreements/quote`, `dns list`.)
    /// Requested at login by default (see
    /// [`crate::environments::DEFAULT_OAUTH_SCOPES`]).
    DOMAINS_READ => "domains.domain:read",
    /// Create/replace/delete DNS records (`dns add/set/delete`).
    DOMAINS_DNS_UPDATE => "domains.dns:update",
    /// Register a domain — the v3 registration-execute step (`domain purchase`).
    DOMAINS_CREATE => "domains.domain:create",
    /// Replace a domain's nameservers (`domain nameservers set`).
    DOMAINS_NAMESERVER_UPDATE => "domains.nameserver:update",

    /// Read Node.js Hosting apps (`hosting nodejs app list/get`).
    HOSTING_APPS_READ => "hosting.paas.apps:read",
    /// Create a Node.js Hosting app (`hosting nodejs app create`).
    HOSTING_APPS_CREATE => "hosting.paas.apps:create",
    /// Update a Node.js Hosting app (`hosting nodejs app update`).
    HOSTING_APPS_UPDATE => "hosting.paas.apps:update",
    /// Delete a Node.js Hosting app (`hosting nodejs app delete`).
    HOSTING_APPS_DELETE => "hosting.paas.apps:delete",
    /// Upload code to a Node.js Hosting app (`hosting nodejs app deploy`/`upload`).
    HOSTING_CODE_WRITE => "hosting.paas.code:write",
    /// Trigger a Node.js Hosting deploy (`hosting nodejs app deploy`).
    HOSTING_DEPLOY_EXECUTE => "hosting.paas.deploy:execute",
    /// Write Node.js Hosting app secrets (`hosting nodejs secret set/delete`).
    HOSTING_SECRETS_WRITE => "hosting.paas.secrets:write",
    /// Read Node.js Hosting app logs (`hosting nodejs app logs`).
    HOSTING_LOGS_READ => "hosting.paas.logs:read",
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every registered scope is well-formed and listed at most once — a
    /// duplicate usually means a copy-paste slip that would misrepresent the set
    /// the OAuth client must be registered for.
    #[test]
    fn all_scopes_are_unique_and_wellformed() {
        // `ALL` is non-empty by construction (the macro requires one or more
        // entries), so this loop always runs at least once.
        let mut seen = std::collections::HashSet::new();
        for scope in ALL {
            assert!(
                seen.insert(*scope),
                "duplicate scope in scopes::ALL: {scope:?}"
            );
            let split = scope.split_once(':');
            assert!(split.is_some(), "scope {scope:?} must be `resource:action`");
            let (resource, action) = split.expect("checked non-None above");
            assert!(
                !resource.is_empty() && !action.is_empty(),
                "malformed scope {scope:?} (expected `resource:action`)"
            );
        }
    }
}
