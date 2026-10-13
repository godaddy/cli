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
//! they're declared outside [`declare_scopes!`]/`ALL` (and listed in
//! [`DIRECTIVES`]) but still need the same server-side registration. When
//! syncing the OAuth client's configuration, diff against `ALL` *plus*
//! [`DIRECTIVES`], not `ALL` alone.
//!
//! # Adding a scope (READ THIS)
//!
//! 1. Add a constant to the [`declare_scopes!`] block below (or, for a directive
//!    scope that isn't a `resource:action` permission, declare it standalone
//!    like [`OFFLINE_ACCESS`], and add it to [`DIRECTIVES`]). Constants in `declare_scopes!` are automatically
//!    included in [`ALL`] — you cannot add one there without registering it in
//!    the list.
//! 2. Reference the new constant from the command via `.with_scopes(&[scopes::…])`.
//! 3. Add its [`SCOPE_REGISTRY`] entry and set `default`. `true` means it is
//!    requested at login; `false` means step-up only, requested when a command
//!    that declares it runs. Use `false` only when the action spends money,
//!    can take a site offline, deletes a whole billable resource, or is raw
//!    data access. The login request ([`DEFAULT_OAUTH_SCOPES`]) is derived
//!    from these flags, so there is nothing else to keep in sync.
//! 4. **Register the same scope on the CLI's OAuth client**, or it will be
//!    ungrantable at runtime.
//!
//! This registry covers the scopes the CLI requests for *itself* (login defaults
//! plus per-command step-up). It intentionally does NOT cover scopes that are
//! user data rather than the CLI's own grants — e.g. the `authorizationScopes`
//! assigned to a third-party app created via `gddy platform app init`, or the
//! ad-hoc scopes `gddy api call --scope …` derives from the API catalog at
//! runtime.

use std::sync::LazyLock;

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
        /// the directive scopes in [`DIRECTIVES`] (e.g. [`OFFLINE_ACCESS`]) aren't
        /// `resource:action` permissions and are deliberately excluded, but still must
        /// be registered on the client server-side. Diff the client's configuration
        /// against `ALL` *plus* [`DIRECTIVES`], not `ALL` alone.
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
/// [`DEFAULT_OAUTH_SCOPES`]).
///
/// Deliberately declared outside [`declare_scopes!`]/[`ALL`]: unlike the
/// scopes below, it isn't a `resource:action` permission grant, so it fails
/// the `resource:action` shape the module tests enforce for `ALL`. It still
/// must be registered on the CLI's OAuth client server-side, or the
/// authorization server will refuse or silently drop it just like any other
/// unregistered scope.
pub const OFFLINE_ACCESS: &str = "offline_access";

/// OIDC directive scope marking the request as an identity (not just
/// authorization) request — some IDPs only populate profile claims when this
/// is present alongside [`PROFILE`], even though `profile` alone is meant to
/// carry the claims.
pub const OPENID: &str = "openid";

/// OIDC directive scope requesting standard profile claims (`name`,
/// `given_name`, `preferred_username`, etc.) on the token.
pub const PROFILE: &str = "profile";

/// Every standalone directive scope — the scopes that require OAuth client
/// registration but, not being `resource:action` permissions, live outside
/// [`ALL`]. Diff the client's configuration against `ALL` *plus* this list.
pub const DIRECTIVES: &[&str] = &[OFFLINE_ACCESS, OPENID, PROFILE];

// DON'T FORGET! If you add a scope here, you must also register it on the CLI's OAuth client.
// Every scope below is requested at login by default unless its SCOPE_REGISTRY
// entry has `default: false` (its doc says "Step-up only").
declare_scopes! {
    /// Read the caller's registered applications.
    APP_REGISTRY_READ => "apps.app-registry:read",
    /// Create/update/archive the caller's registered applications. Declared by
    /// the app-registry mutation commands (`platform app init/update/enable/
    /// disable/archive/release/deploy`); read commands such as
    /// `platform app enablement` use [`APP_REGISTRY_READ`] only.
    APP_REGISTRY_WRITE => "apps.app-registry:write",

    /// Read domains, availability, suggestions, quotes, and DNS records.
    /// (`domain list/get/available/suggest/agreements/quote`, `dns list`.)
    DOMAINS_READ => "domains.domain:read",
    /// Create/replace/delete DNS records (`dns add/set/delete`).
    DOMAINS_DNS_UPDATE => "domains.dns:update",
    /// Register a domain — the v3 registration-execute step (`domain purchase`).
    /// Step-up only: spends money.
    DOMAINS_CREATE => "domains.domain:create",
    /// Replace a domain's nameservers (`domain nameserver set`). Step-up only:
    /// can take a live site offline.
    DOMAINS_NAMESERVER_UPDATE => "domains.nameserver:update",

    /// Read hosting applications (`hosting app list/get/status`).
    HOSTING_APPLICATION_READ => "hosting.application:read",
    /// Create a hosting application (`hosting app create`).
    HOSTING_APPLICATION_CREATE => "hosting.application:create",
    /// Update a hosting application (`hosting app update`).
    HOSTING_APPLICATION_UPDATE => "hosting.application:update",
    /// Delete a hosting application (`hosting app delete`). Step-up only:
    /// deletes a whole billable resource.
    HOSTING_APPLICATION_DELETE => "hosting.application:delete",
    /// Trigger deployments and restarts (`hosting app restart`, `hosting deployment publish`).
    HOSTING_DEPLOYMENT_EXECUTE => "hosting.deployment:execute",
    /// Read source imports (`hosting import get`).
    HOSTING_SOURCE_READ => "hosting.source:read",
    /// Create a source import (`hosting import create`).
    HOSTING_SOURCE_WRITE => "hosting.source:write",
    /// Read application secrets (`hosting secret list`).
    HOSTING_SECRET_READ => "hosting.secret:read",
    /// Write application secrets (`hosting secret create/update/delete/sync`).
    HOSTING_SECRET_WRITE => "hosting.secret:write",
    /// Read application logs (`hosting log list`).
    HOSTING_LOG_READ => "hosting.log:read",
    /// Read application domains (`hosting domain list/get`).
    HOSTING_DOMAIN_READ => "hosting.domain:read",
    /// Attach or detach application domains (`hosting domain attach/detach`).
    HOSTING_DOMAIN_WRITE => "hosting.domain:write",
    /// Read hosting subscriptions (`hosting subscription list/get`).
    HOSTING_SUBSCRIPTION_READ => "hosting.subscription:read",
    /// Attach an an application to a hosting plan (`hosting subscription attach`).
    HOSTING_SUBSCRIPTION_WRITE => "hosting.subscription:write",
    /// Open a raw MySQL-over-WebSocket tunnel to a hosting app's database
    /// (`db tunnel`). A dedicated grant, separate from
    /// [`HOSTING_DEPLOYMENT_EXECUTE`]: authority to publish a deployment does
    /// not by itself grant raw database read/write. Step-up only: raw data access.
    HOSTING_DATABASE_TUNNEL_EXECUTE => "hosting.database.tunnel:execute",

    /// Read mailboxes and check mailbox-creation eligibility (`email list`).
    EMAIL_READ => "email.mailbox:read",
    /// Create a mailbox (`email create`).
    EMAIL_CREATE => "email.mailbox:create",

    /// Trigger an on-demand malware scan (`malware scan run`).
    SECURITY_MALWARE_SCAN => "security.website-security-scan:execute",
    /// Browse and resolve Shopping catalog products.
    SHOPPING_CATALOG_READ => "shopping.catalog:read",
    /// Create, update, read, and complete Shopping checkout sessions. Step-up
    /// only: spends money.
    SHOPPING_CHECKOUT_EXECUTE => "shopping.checkout:execute",
    /// Read completed Shopping orders. Step-up only, granted alongside
    /// [`SHOPPING_CHECKOUT_EXECUTE`] so the order read after a purchase is covered.
    SHOPPING_ORDER_READ => "shopping.order:read",
}

/// A requestable scope, its human description, and whether it is requested at
/// login by default. Backs `gddy auth scope`. Descriptions are hand-ported
/// from the doc comments above — the only part of this table that can't be
/// derived at runtime; which commands use a scope is instead derived live via
/// [`command_scopes`].
pub struct ScopeInfo {
    pub scope: &'static str,
    pub description: &'static str,
    pub default: bool,
}

/// Every requestable scope, described for `gddy auth scope`, and the single
/// source of truth for which scopes the login request carries: entries with
/// `default: true` make up [`DEFAULT_OAUTH_SCOPES`]; `default: false` entries
/// are step-up only and are requested when a command that declares them runs.
///
/// A scope is step-up only when the action it exists for spends money
/// ([`DOMAINS_CREATE`], [`SHOPPING_CHECKOUT_EXECUTE`] plus the
/// [`SHOPPING_ORDER_READ`] that follows a purchase), can take a site offline
/// ([`DOMAINS_NAMESERVER_UPDATE`]), deletes a whole billable resource
/// ([`HOSTING_APPLICATION_DELETE`]), or is raw data access
/// ([`HOSTING_DATABASE_TUNNEL_EXECUTE`]). Editing scopes that also permit
/// deleting a sub-resource (`dns delete`, `hosting secret delete`) stay
/// defaults: the authorization server offers no finer split, and prompting
/// for `domains.dns:update` would put consent back into every site setup.
///
/// `scope_registry_covers_every_declared_scope` keeps this table in sync with
/// the constants above.
pub const SCOPE_REGISTRY: &[ScopeInfo] = &[
    ScopeInfo {
        scope: APP_REGISTRY_READ,
        description: "Read the caller's registered applications",
        default: true,
    },
    ScopeInfo {
        scope: APP_REGISTRY_WRITE,
        description: "Create, update, or archive the caller's registered applications",
        default: true,
    },
    ScopeInfo {
        scope: DOMAINS_READ,
        description: "Read domains, availability, suggestions, quotes, and DNS records",
        default: true,
    },
    ScopeInfo {
        scope: DOMAINS_DNS_UPDATE,
        description: "Create, replace, or delete DNS records",
        default: true,
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
        scope: HOSTING_APPLICATION_READ,
        description: "Read hosting applications",
        default: true,
    },
    ScopeInfo {
        scope: HOSTING_APPLICATION_CREATE,
        description: "Create a hosting application",
        default: true,
    },
    ScopeInfo {
        scope: HOSTING_APPLICATION_UPDATE,
        description: "Update a hosting application",
        default: true,
    },
    ScopeInfo {
        scope: HOSTING_APPLICATION_DELETE,
        description: "Delete a hosting application",
        default: false,
    },
    ScopeInfo {
        scope: HOSTING_DEPLOYMENT_EXECUTE,
        description: "Trigger deployments and restarts",
        default: true,
    },
    ScopeInfo {
        scope: HOSTING_SOURCE_READ,
        description: "Read source imports for hosting applications",
        default: true,
    },
    ScopeInfo {
        scope: HOSTING_SOURCE_WRITE,
        description: "Create source imports for hosting applications",
        default: true,
    },
    ScopeInfo {
        scope: HOSTING_SECRET_READ,
        description: "Read application secret names",
        default: true,
    },
    ScopeInfo {
        scope: HOSTING_SECRET_WRITE,
        description: "Create, update, or delete application secrets",
        default: true,
    },
    ScopeInfo {
        scope: HOSTING_LOG_READ,
        description: "Read application logs",
        default: true,
    },
    ScopeInfo {
        scope: HOSTING_DOMAIN_READ,
        description: "Read domains attached to hosting applications",
        default: true,
    },
    ScopeInfo {
        scope: HOSTING_DOMAIN_WRITE,
        description: "Attach or detach domains from hosting applications",
        default: true,
    },
    ScopeInfo {
        scope: HOSTING_SUBSCRIPTION_READ,
        description: "Read hosting subscriptions",
        default: true,
    },
    ScopeInfo {
        scope: HOSTING_SUBSCRIPTION_WRITE,
        description: "Attach a hosting subscription to an application",
        default: true,
    },
    ScopeInfo {
        scope: HOSTING_DATABASE_TUNNEL_EXECUTE,
        description: "Open a MySQL tunnel to a Node.js Hosting app's database",
        default: false,
    },
    ScopeInfo {
        scope: EMAIL_READ,
        description: "Read mailboxes and check mailbox-creation eligibility",
        default: true,
    },
    ScopeInfo {
        scope: EMAIL_CREATE,
        description: "Create a mailbox",
        default: true,
    },
    ScopeInfo {
        scope: SECURITY_MALWARE_SCAN,
        description: "Trigger an on-demand malware scan",
        default: true,
    },
    ScopeInfo {
        scope: SHOPPING_CATALOG_READ,
        description: "Browse and resolve Shopping catalog products",
        default: true,
    },
    ScopeInfo {
        scope: SHOPPING_CHECKOUT_EXECUTE,
        description: "Create, update, read, and complete Shopping checkout sessions",
        default: false,
    },
    ScopeInfo {
        scope: SHOPPING_ORDER_READ,
        description: "Read completed Shopping orders",
        default: false,
    },
    ScopeInfo {
        scope: OFFLINE_ACCESS,
        description: "Request a refresh token",
        default: true,
    },
    ScopeInfo {
        scope: OPENID,
        description: "Mark the request as an OIDC identity request",
        default: true,
    },
    ScopeInfo {
        scope: PROFILE,
        description: "Request standard OIDC profile claims on the token",
        default: true,
    },
];

/// Scopes requested at login: every [`SCOPE_REGISTRY`] entry with
/// `default: true`, in registry order. Each scope step-up is a browser round
/// trip through the consent screen, so the login grant is wide on purpose: a
/// first-time session that buys a domain, buys hosting and deploys a site sees
/// a consent screen at login and at each purchase, and nowhere else.
pub static DEFAULT_OAUTH_SCOPES: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    SCOPE_REGISTRY
        .iter()
        .filter(|info| info.default)
        .map(|info| info.scope)
        .collect()
});

/// Every leaf command's space-separated invocation path (e.g. `"domain
/// purchase"`) paired with the scopes it declares via `.with_scopes`.
///
/// Derived by walking each module's real command tree via
/// [`cli_engine::build_module_group`] — the same tree `main` mounts — so this
/// can never drift from what the CLI actually registers. Backs both `gddy
/// auth scope` and [`tests::every_permission_scope_is_wired_to_a_command`].
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

/// Maps every alias spelling of a command's invocation path (plus the
/// canonical path itself) to that canonical path — e.g. both `"domain
/// agreements"` and `"domain agreement"` map to `"domain agreement"`.
///
/// `--command` on `gddy auth scope` otherwise only recognizes the exact
/// canonical path [`command_scopes`] derives from `group.name`/`spec.name`:
/// without this, a `--command` value spelled with a hidden plural alias
/// (one still accepted by the real parser) would be wrongly rejected as
/// unknown here.
pub(crate) fn command_path_aliases() -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    for module in crate::all_modules() {
        walk_group_aliases(
            &cli_engine::build_module_group(&module),
            Vec::new(),
            vec![Vec::new()],
            &mut out,
        );
    }
    out
}

fn walk_group_aliases(
    group: &cli_engine::RuntimeGroupSpec,
    canonical_prefix: Vec<String>,
    alias_prefixes: Vec<Vec<String>>,
    out: &mut std::collections::HashMap<String, String>,
) {
    let mut canonical = canonical_prefix;
    canonical.push(group.group.name.clone());

    let mut group_names = vec![group.group.name.clone()];
    group_names.extend(group.group.aliases.iter().cloned());

    let mut new_alias_prefixes = Vec::new();
    for prefix in &alias_prefixes {
        for name in &group_names {
            let mut variant = prefix.clone();
            variant.push(name.clone());
            new_alias_prefixes.push(variant);
        }
    }

    for command in &group.commands {
        let mut canonical_cmd = canonical.clone();
        canonical_cmd.push(command.spec.name.clone());
        let canonical_path = canonical_cmd.join(" ");

        let mut cmd_names = vec![command.spec.name.clone()];
        cmd_names.extend(command.spec.aliases.iter().cloned());

        for prefix in &new_alias_prefixes {
            for cmd_name in &cmd_names {
                let mut variant = prefix.clone();
                variant.push(cmd_name.clone());
                out.insert(variant.join(" "), canonical_path.clone());
            }
        }
    }

    for sub in &group.groups {
        walk_group_aliases(sub, canonical.clone(), new_alias_prefixes.clone(), out);
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
    /// registration — `ALL` plus the standalone [`DIRECTIVES`] — or `gddy auth scope` would silently omit a
    /// scope an agent needs to plan an eager login around.
    #[test]
    fn scope_registry_covers_every_declared_scope() {
        let registered: std::collections::HashSet<&str> =
            SCOPE_REGISTRY.iter().map(|info| info.scope).collect();
        for scope in ALL.iter().chain(DIRECTIVES).copied() {
            assert!(
                registered.contains(scope),
                "scope {scope:?} is declared but missing from SCOPE_REGISTRY"
            );
        }
    }

    /// Every permission scope in [`ALL`] must be wired to at least one live
    /// command via `.with_scopes`, or it's dead — declared, registered on the
    /// OAuth client and (if a default) requested on every login, yet reachable
    /// by no command. Defaults are not exempt: a default that no command uses
    /// is still shown on every consent screen, so it is the worst kind of
    /// stale entry.
    #[test]
    fn every_permission_scope_is_wired_to_a_command() {
        let used: std::collections::HashSet<String> = command_scopes()
            .into_iter()
            .flat_map(|(_, scopes)| scopes)
            .collect();
        for scope in ALL {
            assert!(
                used.contains(*scope),
                "scope {scope:?} is declared but no command declares it via .with_scopes"
            );
        }
    }

    /// [`SCOPE_REGISTRY`] must not list a scope twice, or `gddy auth scope`
    /// would show it twice and [`DEFAULT_OAUTH_SCOPES`] would request it twice.
    #[test]
    fn scope_registry_entries_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for info in SCOPE_REGISTRY {
            assert!(
                seen.insert(info.scope),
                "duplicate scope in SCOPE_REGISTRY: {:?}",
                info.scope
            );
        }
    }

    /// The login request is derived from the registry's `default` flags, so it
    /// must be exactly those entries and must always carry [`OFFLINE_ACCESS`]
    /// (without it no refresh token is issued and every session re-prompts).
    #[test]
    fn default_oauth_scopes_are_the_registry_defaults() {
        let by_scope: std::collections::HashMap<&str, bool> = SCOPE_REGISTRY
            .iter()
            .map(|info| (info.scope, info.default))
            .collect();
        for scope in DEFAULT_OAUTH_SCOPES.iter() {
            assert_eq!(
                by_scope.get(scope),
                Some(&true),
                "{scope:?} is requested at login but is not a `default: true` registry entry"
            );
        }
        let default_count = by_scope.values().filter(|default| **default).count();
        assert_eq!(
            DEFAULT_OAUTH_SCOPES.len(),
            default_count,
            "a default entry is missing or duplicated"
        );
        assert!(DEFAULT_OAUTH_SCOPES.contains(&OFFLINE_ACCESS));
        assert!(!DEFAULT_OAUTH_SCOPES.contains(&DOMAINS_CREATE));
        assert!(!DEFAULT_OAUTH_SCOPES.contains(&SHOPPING_CHECKOUT_EXECUTE));
    }

    /// The step-up-only set is a deliberate policy decision (spends money,
    /// takes a site offline, deletes a billable resource, or raw data access),
    /// so pin it: flipping a `default` flag either way must be an explicit
    /// change here, not a side effect of editing the registry.
    #[test]
    fn step_up_only_scopes_are_exactly_the_sensitive_ones() {
        let non_default: std::collections::BTreeSet<&str> = SCOPE_REGISTRY
            .iter()
            .filter(|info| !info.default)
            .map(|info| info.scope)
            .collect();
        let expected = std::collections::BTreeSet::from([
            DOMAINS_CREATE,
            DOMAINS_NAMESERVER_UPDATE,
            HOSTING_APPLICATION_DELETE,
            HOSTING_DATABASE_TUNNEL_EXECUTE,
            SHOPPING_CHECKOUT_EXECUTE,
            SHOPPING_ORDER_READ,
        ]);
        assert_eq!(non_default, expected);
    }
}
