use super::*;

/// Every registered scope is well-formed and listed at most once — a
/// duplicate usually means a copy-paste slip that would misrepresent the set
/// the OAuth client must be registered for, and would show twice in `gddy
/// auth scope` and be requested twice at login.
#[test]
fn all_scopes_are_unique_and_wellformed() {
    // `ALL` is non-empty by construction (the macro requires one or more
    // entries), so this loop always runs at least once.
    for scope in ALL {
        let split = scope.split_once(':');
        assert!(split.is_some(), "scope {scope:?} must be `resource:action`");
        let (resource, action) = split.expect("checked non-None above");
        assert!(
            !resource.is_empty() && !action.is_empty(),
            "malformed scope {scope:?} (expected `resource:action`)"
        );
    }
    let mut seen = std::collections::HashSet::new();
    for info in SCOPE_REGISTRY {
        assert!(
            seen.insert(info.scope),
            "duplicate scope in SCOPE_REGISTRY: {:?}",
            info.scope
        );
    }
    assert_eq!(
        SCOPE_REGISTRY.len(),
        ALL.len() + DIRECTIVES.len(),
        "SCOPE_REGISTRY must be exactly ALL plus DIRECTIVES"
    );
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

/// The login request is generated alongside the registry, so it must be
/// exactly the registry's `default: true` entries in registry order, and must
/// always carry [`OFFLINE_ACCESS`] (without it no refresh token is issued and
/// every session re-prompts).
#[test]
fn default_oauth_scopes_are_the_registry_defaults() {
    let expected: Vec<&str> = SCOPE_REGISTRY
        .iter()
        .filter(|info| info.default)
        .map(|info| info.scope)
        .collect();
    assert_eq!(DEFAULT_OAUTH_SCOPES, expected.as_slice());
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
