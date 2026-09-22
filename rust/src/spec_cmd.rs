//! `gddy spec`: emits the full command spec (flags, output schema) as JSON.
//!
//! Feeds DEVEX-1035's dev-portal automation (DEVEX-1038). Kept separate from
//! `gddy tree`, which stays small for quick discovery; `spec` is the heavier,
//! explicitly-invoked walk meant for external tooling.
//!
//! Uses `cli_engine::build_module_group`, the same function
//! `crate::scopes::command_scopes` uses, so the output can't drift from the
//! real CLI and needs no cli-engine changes.
//!
//! Scoped to product commands (plus `auth`, needed to use them). Deliberately
//! excludes cli-engine's own meta commands (`tree`, `guide`, `help`,
//! `completion`, `search`, `flags`, and `spec` itself): none of DEVEX-1035's
//! consumers document CLI mechanics, only GoDaddy platform functionality.

use clap::{Arg, ArgAction};
use cli_engine::{
    CommandResult, CommandSpec, RuntimeCommandSpec, RuntimeGroupSpec, SchemaInfo, Tier,
};
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::json;

#[derive(Debug, Clone, Serialize, JsonSchema)]
struct SpecFlag {
    #[serde(skip_serializing_if = "Option::is_none")]
    long: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    short: Option<char>,
    #[serde(skip_serializing_if = "Option::is_none")]
    help: Option<String>,
    /// True when clap requires this flag unconditionally. False doesn't
    /// mean never required; see `required_if`.
    required: bool,
    /// Conditional requirement note (e.g. "required when --type is CAA").
    /// clap has no public way to expose `required_if_eq`, so this comes from
    /// a hand-maintained table; see `dns::records::CONDITIONALLY_REQUIRED_FLAGS`.
    #[serde(skip_serializing_if = "Option::is_none")]
    required_if: Option<&'static str>,
    takes_value: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
struct SpecNode {
    name: String,
    path: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    long: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    flags: Vec<SpecFlag>,
    /// `SchemaInfo` doesn't implement `JsonSchema` (it carries an arbitrary
    /// nested JSON Schema itself), so its schema is generated as a generic
    /// JSON value rather than a fully-typed shape.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<serde_json::Value>")]
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

/// Looks up a hand-maintained conditional-requirement note for a flag, if
/// any. See `SpecFlag::required_if` for why this isn't derived from clap.
fn conditional_requirement_note(path: &str, long: &str) -> Option<&'static str> {
    match path {
        "dns add" | "dns set" => crate::dns::records::CONDITIONALLY_REQUIRED_FLAGS
            .iter()
            .find(|(flag, _)| *flag == long)
            .map(|(_, note)| *note),
        _ => None,
    }
}

fn build_flags(path: &str, args: &[Arg]) -> Vec<SpecFlag> {
    args.iter()
        .filter(|arg| !arg.is_positional())
        .map(|arg| {
            let long = arg.get_long().map(ToString::to_string);
            let required_if = long
                .as_deref()
                .and_then(|long| conditional_requirement_note(path, long));
            SpecFlag {
                long,
                short: arg.get_short(),
                help: arg.get_help().map(ToString::to_string),
                required: arg.is_required_set(),
                required_if,
                takes_value: arg_requires_value(arg),
            }
        })
        .collect()
}

fn build_command_node(path: &str, spec: &CommandSpec) -> SpecNode {
    SpecNode {
        name: spec.name.clone(),
        path: path.to_owned(),
        description: spec.short.clone(),
        long: spec.long.clone(),
        flags: build_flags(path, &spec.args),
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

/// Builds the full spec: every module, plus `auth`, which isn't a module and
/// has to be rebuilt by hand to match `main.rs`'s auth config ("godaddy").
fn build_spec_tree() -> Vec<SpecNode> {
    let auth_group = cli_engine::auth_command_group("godaddy", &["godaddy".to_owned()])
        .with_command(crate::scopes_cmd::auth_scopes_command());
    let mut nodes = vec![build_group_node("auth", &auth_group)];
    nodes.extend(crate::all_modules().iter().map(|module| {
        let group = cli_engine::build_module_group(module);
        build_group_node(&group.group.name.clone(), &group)
    }));
    nodes
}

pub(crate) fn spec_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new(
        CommandSpec::new(
            "spec",
            "Emit the full command spec (flags, output schema) as JSON for external tooling",
        )
        .with_tier(Tier::Read)
        .no_auth(true)
        .with_json_schema::<Vec<SpecNode>>(),
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

    /// Guards against the derived `JsonSchema` drifting from the real
    /// `SpecNode`/`SpecFlag` output shape — the artifact the dev-portal team
    /// wants assurance against (DEVEX-1038 review).
    #[tokio::test]
    async fn spec_output_conforms_to_its_own_json_schema() {
        let schema_output = cli().run(["gddy", "spec", "--schema"]).await;
        assert_eq!(
            schema_output.exit_code, 0,
            "rendered output: {}",
            schema_output.rendered
        );
        let schema_payload: serde_json::Value =
            serde_json::from_str(&schema_output.rendered).expect("valid json output");
        let schema = schema_payload["data"]["schema"]
            .as_object()
            .expect("spec command should register a full JSON schema")
            .clone();

        let data_output = cli().run(["gddy", "spec", "--output", "json"]).await;
        assert_eq!(
            data_output.exit_code, 0,
            "rendered output: {}",
            data_output.rendered
        );
        let data_payload: serde_json::Value =
            serde_json::from_str(&data_output.rendered).expect("valid json output");

        let validator = jsonschema::validator_for(&serde_json::Value::Object(schema))
            .expect("derived spec schema should itself be a valid JSON Schema document");
        let errors: Vec<String> = validator
            .iter_errors(&data_payload["data"])
            .map(|error| error.to_string())
            .collect();
        assert!(
            errors.is_empty(),
            "spec output should conform to its own derived schema: {errors:?}"
        );
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
        for expected in ["auth", "domain", "dns", "pat", "payment-methods"] {
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

    /// Confirms the walk reaches leaf command flags, not just names.
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

    /// `auth` isn't in `all_modules()`, so it needs its own coverage.
    #[tokio::test]
    async fn spec_publishes_the_auth_group_and_its_commands() {
        let output = cli().run(["gddy", "spec", "--output", "json"]).await;
        assert_eq!(output.exit_code, 0, "rendered output: {}", output.rendered);

        let payload: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json output");
        let auth = payload["data"]
            .as_array()
            .expect("data array")
            .iter()
            .find(|node| node["name"] == "auth")
            .expect("auth group present");
        let names: Vec<&str> = auth["children"]
            .as_array()
            .expect("auth children array")
            .iter()
            .filter_map(|node| node["name"].as_str())
            .collect();
        for expected in ["login", "status", "logout", "scopes"] {
            assert!(
                names.contains(&expected),
                "auth should publish {expected:?} among its commands: {names:?}"
            );
        }
    }

    /// `--tag`/`--usage` are only required for CAA/TLSA; clap can't expose
    /// that, so `required_if` must carry it instead.
    #[tokio::test]
    async fn spec_surfaces_conditional_requirements_clap_cannot_expose() {
        let output = cli().run(["gddy", "spec", "--output", "json"]).await;
        assert_eq!(output.exit_code, 0, "rendered output: {}", output.rendered);

        let payload: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json output");
        let dns = payload["data"]
            .as_array()
            .expect("data array")
            .iter()
            .find(|node| node["name"] == "dns")
            .expect("dns module present");
        let add = dns["children"]
            .as_array()
            .expect("dns children array")
            .iter()
            .find(|node| node["name"] == "add")
            .expect("dns add present");
        let flags = add["flags"].as_array().expect("dns add flags array");

        let tag = flags
            .iter()
            .find(|flag| flag["long"] == "tag")
            .expect("--tag flag present");
        assert_eq!(
            tag["required"], false,
            "--tag isn't unconditionally required"
        );
        assert!(
            tag["required_if"]
                .as_str()
                .is_some_and(|note| note.contains("CAA")),
            "--tag should carry its CAA conditional requirement: {tag}"
        );

        let usage = flags
            .iter()
            .find(|flag| flag["long"] == "usage")
            .expect("--usage flag present");
        assert!(
            usage["required_if"]
                .as_str()
                .is_some_and(|note| note.contains("TLSA")),
            "--usage should carry its TLSA conditional requirement: {usage}"
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
