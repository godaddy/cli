//! Live view of which commands declare which scopes, derived from the real
//! command tree.

/// Every leaf command's space-separated invocation path (e.g. `"domain
/// purchase"`) paired with the scopes it declares via `.with_scopes`.
///
/// Derived by walking each module's real command tree via
/// [`cli_engine::build_module_group`] — the same tree `main` mounts — so this
/// can never drift from what the CLI actually registers. Backs both `gddy
/// auth scope` and the `every_permission_scope_is_wired_to_a_command` test.
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
