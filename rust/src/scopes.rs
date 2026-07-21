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
//! slice is derived from the same declarations, so it is always the complete
//! list of `resource:action` *permission* scopes the OAuth client must be
//! registered for — a single place to diff against the client's configuration.
//!
//! `ALL` does NOT cover every scope requiring client registration, though:
//! directive scopes like [`OFFLINE_ACCESS`] aren't `resource:action` grants, so
//! they're declared outside [`declare_scopes!`]/`ALL` but still need the same
//! server-side registration. When syncing the OAuth client's configuration,
//! diff against `ALL` *plus* every such standalone constant, not `ALL` alone.
//!
//! # Adding a scope (READ THIS)
//!
//! 1. Add a constant to the [`declare_scopes!`] block below (or, for a directive
//!    scope that isn't a `resource:action` permission, declare it standalone
//!    like [`OFFLINE_ACCESS`]). Constants in `declare_scopes!` are automatically
//!    included in [`ALL`] — you cannot add one there without registering it in
//!    the list.
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
    /// Upload code to a Node.js Hosting app (`hosting nodejs source upload`).
    HOSTING_CODE_WRITE => "hosting.paas.code:write",
    /// Poll a git import job (`hosting nodejs source git-status`).
    HOSTING_CODE_READ => "hosting.paas.code:read",
    /// Trigger a Node.js Hosting deploy (`hosting nodejs deployment publish`).
    HOSTING_DEPLOY_EXECUTE => "hosting.paas.deploy:execute",
    /// Manage GitHub connections and browse repos/branches (`hosting nodejs github *`).
    HOSTING_GITHUB_EXECUTE => "hosting.paas.github:execute",
    /// Write Node.js Hosting app secrets (`hosting nodejs secret set/delete`).
    HOSTING_SECRETS_WRITE => "hosting.paas.secrets:write",
    /// Read Node.js Hosting app logs (`hosting nodejs app logs`).
    HOSTING_LOGS_READ => "hosting.paas.logs:read",
}

/// A requestable scope, its human description, and whether it is requested at
/// login by default. Backs `gddy auth scopes`. Descriptions are hand-ported
/// from the doc comments above — the only part of this table that can't be
/// derived at runtime; which commands use a scope is instead derived live via
/// [`command_scopes`].
pub struct ScopeInfo {
    pub scope: &'static str,
    pub description: &'static str,
    pub default: bool,
}

/// Every requestable scope, described for `gddy auth scopes`. Keep in sync
/// with the constants above (`scope_registry_covers_every_declared_scope`
/// enforces the constant side of this; `scope_registry_default_flag_matches_default_oauth_scopes`
/// enforces that the `default` flag exactly matches
/// [`crate::environments::DEFAULT_OAUTH_SCOPES`]).
pub const SCOPE_REGISTRY: &[ScopeInfo] = &[
    ScopeInfo {
        scope: APP_REGISTRY_READ,
        description: "Read the caller's registered applications",
        default: true,
    },
    ScopeInfo {
        scope: APP_REGISTRY_WRITE,
        description: "Create, update, or archive the caller's registered applications",
        default: false,
    },
    ScopeInfo {
        scope: DOMAINS_READ,
        description: "Read domains, availability, suggestions, quotes, and DNS records",
        default: true,
    },
    ScopeInfo {
        scope: DOMAINS_DNS_UPDATE,
        description: "Create, replace, or delete DNS records",
        default: false,
    },
    ScopeInfo {
        scope: DOMAINS_CREATE,
        description: "Register a domain",
        default: false,
    },
    ScopeInfo {
        scope: DOMAINS_NAMESERVER_UPDATE,
        description: "Replace a domain's nameservers",
        default: false,
    },
    ScopeInfo {
        scope: HOSTING_APPS_READ,
        description: "Read Node.js Hosting apps",
        default: false,
    },
    ScopeInfo {
        scope: HOSTING_APPS_CREATE,
        description: "Create a Node.js Hosting app",
        default: false,
    },
    ScopeInfo {
        scope: HOSTING_APPS_UPDATE,
        description: "Update a Node.js Hosting app",
        default: false,
    },
    ScopeInfo {
        scope: HOSTING_APPS_DELETE,
        description: "Delete a Node.js Hosting app",
        default: false,
    },
    ScopeInfo {
        scope: HOSTING_CODE_WRITE,
        description: "Upload code to a Node.js Hosting app",
        default: false,
    },
    ScopeInfo {
        scope: HOSTING_CODE_READ,
        description: "View source files, database contents, and git history for your Node.js Hosting apps",
        default: false,
    },
    ScopeInfo {
        scope: HOSTING_DEPLOY_EXECUTE,
        description: "Trigger a Node.js Hosting deploy",
        default: false,
    },
    ScopeInfo {
        scope: HOSTING_SECRETS_WRITE,
        description: "Write Node.js Hosting app secrets",
        default: false,
    },
    ScopeInfo {
        scope: HOSTING_LOGS_READ,
        description: "Read Node.js Hosting app logs",
        default: false,
    },
    ScopeInfo {
        scope: HOSTING_GITHUB_EXECUTE,
        description: "Connect GitHub and import code for your Node.js Hosting apps",
        default: false,
    },
    ScopeInfo {
        scope: OFFLINE_ACCESS,
        description: "Request a refresh token",
        default: true,
    },
];

/// Every leaf command's space-separated invocation path (e.g. `"domain
/// purchase"`) paired with the scopes it declares via `.with_scopes`.
///
/// Derived by walking each module's real command tree via
/// [`cli_engine::build_module_group`] — the same tree `main` mounts — so this
/// can never drift from what the CLI actually registers. Backs both `gddy
/// auth scopes` and [`tests::scope_registry_non_default_entries_are_wired_to_a_command`].
pub(crate) fn command_scopes() -> Vec<(String, Vec<String>)> {
    let mut out = Vec::new();
    for module in crate::all_modules() {
        walk_group(
            &cli_engine::build_module_group(&module),
            Vec::new(),
            &mut out,
        );
    }
    out
}

fn walk_group(
    group: &cli_engine::RuntimeGroupSpec,
    prefix: Vec<String>,
    out: &mut Vec<(String, Vec<String>)>,
) {
    let mut path = prefix;
    path.push(group.group.name.clone());
    for command in &group.commands {
        let mut cmd_path = path.clone();
        cmd_path.push(command.spec.name.clone());
        out.push((cmd_path.join(" "), command.spec.metadata().scopes.clone()));
    }
    for sub in &group.groups {
        walk_group(sub, path.clone(), out);
    }
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

    /// [`SCOPE_REGISTRY`] must describe every scope requiring OAuth client
    /// registration — `ALL` plus the standalone directive scopes
    /// ([`OFFLINE_ACCESS`]) — or `gddy auth scopes` would silently omit a
    /// scope an agent needs to plan an eager login around.
    #[test]
    fn scope_registry_covers_every_declared_scope() {
        let registered: std::collections::HashSet<&str> =
            SCOPE_REGISTRY.iter().map(|info| info.scope).collect();
        for scope in ALL.iter().copied().chain([OFFLINE_ACCESS]) {
            assert!(
                registered.contains(scope),
                "scope {scope:?} is declared but missing from SCOPE_REGISTRY"
            );
        }
    }

    /// Every non-default scope in [`SCOPE_REGISTRY`] must be wired to at
    /// least one live command via `.with_scopes`, or it's dead — declared and
    /// registered on the OAuth client but unreachable by any command, which
    /// is either a stale entry or a scope some command forgot to declare.
    /// Default scopes are exempt: they're requested at login by default
    /// regardless of which commands request them.
    #[test]
    fn scope_registry_non_default_entries_are_wired_to_a_command() {
        let used: std::collections::HashSet<String> = command_scopes()
            .into_iter()
            .flat_map(|(_, scopes)| scopes)
            .collect();
        for info in SCOPE_REGISTRY {
            assert!(
                info.default || used.contains(info.scope),
                "scope {:?} is in SCOPE_REGISTRY with default: false but no \
                 command declares it via .with_scopes",
                info.scope
            );
        }
    }

    /// [`SCOPE_REGISTRY`]'s `default` flag must exactly match
    /// [`crate::environments::DEFAULT_OAUTH_SCOPES`] — the module doc on
    /// [`SCOPE_REGISTRY`] used to call this out as an unenforced invariant;
    /// this is that check. If they drift, `gddy auth scopes --defaults-only`
    /// would misdescribe which scopes actually get requested at login.
    #[test]
    fn scope_registry_default_flag_matches_default_oauth_scopes() {
        let default_in_registry: std::collections::HashSet<&str> = SCOPE_REGISTRY
            .iter()
            .filter(|info| info.default)
            .map(|info| info.scope)
            .collect();
        let default_oauth_scopes: std::collections::HashSet<&str> =
            crate::environments::DEFAULT_OAUTH_SCOPES
                .iter()
                .copied()
                .collect();
        assert_eq!(
            default_in_registry, default_oauth_scopes,
            "SCOPE_REGISTRY's `default` flags must match \
             crate::environments::DEFAULT_OAUTH_SCOPES exactly"
        );
    }
}
