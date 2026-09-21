//! `gddy spec` — emit the full command spec (flags, output schema) as JSON.
//!
//! Feeds DEVEX-1035's dev-portal reference automation (DEVEX-1038). Kept
//! separate from `gddy tree`: `tree` stays a small, agent-discovery-safe
//! payload (name/path/description only), while `spec` is an explicitly
//! invoked, richer walk meant for external tooling, not routine agent
//! orientation.
//!
//! Walks every module's real command tree via
//! [`cli_engine::build_module_group`] — the same public entry point
//! [`crate::scopes::command_scopes`] already uses — so this can never drift
//! from what the CLI actually registers, with no cli-engine changes needed.

use clap::{Arg, ArgAction};
use cli_engine::{
    CommandResult, CommandSpec, RuntimeCommandSpec, RuntimeGroupSpec, SchemaInfo, Tier,
};
use serde::Serialize;
use serde_json::json;

#[derive(Debug, Clone, Serialize)]
struct SpecFlag {
    #[serde(skip_serializing_if = "Option::is_none")]
    long: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    short: Option<char>,
    #[serde(skip_serializing_if = "Option::is_none")]
    help: Option<String>,
    required: bool,
    takes_value: bool,
}

#[derive(Debug, Clone, Serialize)]
struct SpecNode {
    name: String,
    path: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    long: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    flags: Vec<SpecFlag>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    output: Option<SchemaInfo>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    children: Vec<SpecNode>,
}

fn arg_requires_value(arg: &Arg) -> bool {
    match arg.get_action() {
        ArgAction::Set | ArgAction::Append => arg
            .get_num_args()
            .is_none_or(|range| range.takes_values() && range.min_values() > 0),
        ArgAction::SetTrue
        | ArgAction::SetFalse
        | ArgAction::Count
        | ArgAction::Help
        | ArgAction::HelpShort
        | ArgAction::HelpLong
        | ArgAction::Version => false,
        _ => arg
            .get_num_args()
            .is_some_and(|range| range.takes_values() && range.min_values() > 0),
    }
}

fn build_flags(args: &[Arg]) -> Vec<SpecFlag> {
    args.iter()
        .filter(|arg| !arg.is_positional())
        .map(|arg| SpecFlag {
            long: arg.get_long().map(ToString::to_string),
            short: arg.get_short(),
            help: arg.get_help().map(ToString::to_string),
            required: arg.is_required_set(),
            takes_value: arg_requires_value(arg),
        })
        .collect()
}

fn build_command_node(path: &str, spec: &CommandSpec) -> SpecNode {
    SpecNode {
        name: spec.name.clone(),
        path: path.to_owned(),
        description: spec.short.clone(),
        long: spec.long.clone(),
        flags: build_flags(&spec.args),
        output: spec.output_schema.clone(),
        children: Vec::new(),
    }
}

fn build_group_node(path: &str, group: &RuntimeGroupSpec) -> SpecNode {
    let children = group
        .commands
        .iter()
        .filter(|command| !command.spec.hidden)
        .map(|command| {
            let child_path = format!("{path} {}", command.spec.name);
            build_command_node(&child_path, &command.spec)
        })
        .chain(
            group
                .groups
                .iter()
                .filter(|sub| !sub.group.hidden)
                .map(|sub| {
                    let child_path = format!("{path} {}", sub.group.name);
                    build_group_node(&child_path, sub)
                }),
        )
        .collect();

    SpecNode {
        name: group.group.name.clone(),
        path: path.to_owned(),
        description: group.group.short.clone(),
        long: group.group.long.clone(),
        flags: Vec::new(),
        output: None,
        children,
    }
}

/// Walks every module's real command tree via [`cli_engine::build_module_group`]
/// into a [`SpecNode`] forest, one root per module.
fn build_spec_tree() -> Vec<SpecNode> {
    crate::all_modules()
        .iter()
        .map(|module| {
            let group = cli_engine::build_module_group(module);
            build_group_node(&group.group.name.clone(), &group)
        })
        .collect()
}

pub(crate) fn spec_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new(
        CommandSpec::new(
            "spec",
            "Emit the full command spec (flags, output schema) as JSON for external tooling",
        )
        .with_tier(Tier::Read)
        .no_auth(true),
        |_credential, _args| async move { Ok(CommandResult::new(json!(build_spec_tree()))) },
    )
}

#[cfg(test)]
mod tests {
    use cli_engine::{Cli, CliConfig};
    use serde_json::json;

    use super::spec_command;

    fn cli() -> Cli {
        Cli::new(
            CliConfig::new("gddy", "GoDaddy developer CLI", "gddy")
                .with_modules(crate::all_modules())
                .with_command(spec_command()),
        )
    }

    #[tokio::test]
    async fn spec_runs_without_auth() {
        let output = cli().run(["gddy", "spec", "--output", "json"]).await;
        assert_eq!(output.exit_code, 0, "rendered output: {}", output.rendered);
    }

    #[tokio::test]
    async fn spec_publishes_every_module_with_name_path_and_description() {
        let output = cli().run(["gddy", "spec", "--output", "json"]).await;
        assert_eq!(output.exit_code, 0, "rendered output: {}", output.rendered);

        let payload: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json output");
        let modules = payload["data"].as_array().expect("data array");
        assert!(!modules.is_empty(), "spec should publish top-level modules");

        let names: Vec<&str> = modules
            .iter()
            .filter_map(|node| node["name"].as_str())
            .collect();
        for expected in ["domain", "dns", "pat", "payment-methods"] {
            assert!(
                names.contains(&expected),
                "spec should publish {expected:?} among top-level modules: {names:?}"
            );
        }

        for node in modules {
            for field in ["name", "path", "description"] {
                assert!(
                    node[field].as_str().is_some_and(|s| !s.is_empty()),
                    "every top-level node should have a non-empty {field:?}: {node}"
                );
            }
        }
    }

    /// `domain purchase`'s domain-name positional isn't a flag, but its
    /// `--years`-style options should surface with the right `required`/
    /// `takes_value` shape, proving the walk reaches leaf command flags.
    #[tokio::test]
    async fn spec_surfaces_leaf_command_flags() {
        let output = cli().run(["gddy", "spec", "--output", "json"]).await;
        assert_eq!(output.exit_code, 0, "rendered output: {}", output.rendered);

        let payload: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json output");
        let domain = payload["data"]
            .as_array()
            .expect("data array")
            .iter()
            .find(|node| node["name"] == "domain")
            .expect("domain module present");
        let purchase = domain["children"]
            .as_array()
            .expect("domain children array")
            .iter()
            .find(|node| node["name"] == "purchase")
            .expect("domain purchase present");
        assert!(
            purchase["flags"]
                .as_array()
                .is_some_and(|flags| !flags.is_empty()),
            "domain purchase should publish at least one flag: {purchase}"
        );
    }

    #[test]
    fn build_group_node_excludes_hidden_commands_and_groups() {
        use cli_engine::{CommandSpec, GroupSpec, RuntimeCommandSpec, RuntimeGroupSpec};

        let group = RuntimeGroupSpec::new(GroupSpec::new("parent", "Parent group"))
            .with_command(RuntimeCommandSpec::new(
                CommandSpec::new("visible", "A visible command"),
                |_credential, _args| async move { Ok(cli_engine::CommandResult::new(json!({}))) },
            ))
            .with_command(RuntimeCommandSpec::new(
                CommandSpec::new("secret", "A hidden command").hidden(true),
                |_credential, _args| async move { Ok(cli_engine::CommandResult::new(json!({}))) },
            ))
            .with_group(RuntimeGroupSpec::new(
                GroupSpec::new("hidden-group", "A hidden group").hidden(true),
            ));

        let node = super::build_group_node("parent", &group);
        let names: Vec<&str> = node
            .children
            .iter()
            .map(|child| child.name.as_str())
            .collect();
        assert_eq!(
            names,
            vec!["visible"],
            "hidden command/group leaked: {names:?}"
        );
    }
}
