use serde_json::{Value, json};

#[allow(dead_code)]
pub fn adapt(rendered: &str, command: &str, is_success: bool) -> Option<String> {
    if command == "gddy application deploy" {
        return None;
    }

    let source: Value = serde_json::from_str(rendered).ok()?;
    let next_actions = source
        .get("next_actions")
        .cloned()
        .unwrap_or_else(|| json!([]));

    let public = if is_success {
        json!({
            "ok": true,
            "command": command,
            "result": source.get("data").cloned().unwrap_or(Value::Null),
            "next_actions": next_actions,
        })
    } else {
        let error = source.get("error")?;
        json!({
            "ok": false,
            "command": command,
            "error": {
                "code": error.get("code")?,
                "message": error.get("message")?,
            },
            "next_actions": next_actions,
        })
    };

    let mut output = serde_json::to_string_pretty(&public).ok()?;
    output.push('\n');
    Some(output)
}

#[allow(dead_code)]
pub fn command_path(root: &clap::Command, args: &[std::ffi::OsString]) -> String {
    let mut names = Vec::new();
    let text_args = args
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let normalized = normalize_optional_global_flags(root, &text_args);
    let mut current = root;
    let mut index = usize::from(
        normalized
            .first()
            .is_some_and(|arg| argv0_matches_root(arg, root.get_name())),
    );

    while let Some(arg) = normalized.get(index) {
        if arg == "--" {
            break;
        }

        if let Some(subcommand) = direct_subcommand(current, arg) {
            names.push(subcommand.get_name());
            current = subcommand;
            index += 1;
            continue;
        }

        if arg.starts_with('-') {
            let consumes_value = !arg.contains('=')
                && flag_takes_value(current, root, arg)
                && normalized.get(index + 1).is_some();
            index += if consumes_value { 2 } else { 1 };
            continue;
        }

        if current.get_subcommands().next().is_some() {
            break;
        }
        index += 1;
    }

    if names.is_empty() {
        "gddy".to_owned()
    } else {
        format!("gddy {}", names.join(" "))
    }
}

fn argv0_matches_root(arg: &str, root_name: &str) -> bool {
    arg == root_name
        || std::path::Path::new(arg)
            .file_stem()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name == root_name)
}

fn normalize_optional_global_flags(root: &clap::Command, args: &[String]) -> Vec<String> {
    let mut normalized = Vec::with_capacity(args.len());
    let mut current = root;
    let mut index = 0;

    while let Some(arg) = args.get(index) {
        if matches!(arg.as_str(), "--debug" | "--verbose") {
            let default = if arg == "--debug" { "*" } else { "all" };
            let next = args.get(index + 1);
            if current.get_name() == root.get_name()
                || next.is_none_or(|value| {
                    value.starts_with('-') || direct_subcommand(current, value).is_some()
                })
            {
                normalized.push(format!("{arg}={default}"));
                index += 1;
                continue;
            }
        }

        normalized.push(arg.clone());
        if !arg.starts_with('-')
            && let Some(subcommand) = direct_subcommand(current, arg)
        {
            current = subcommand;
        }
        index += 1;
    }

    normalized
}

fn direct_subcommand<'command>(
    command: &'command clap::Command,
    token: &str,
) -> Option<&'command clap::Command> {
    command.get_subcommands().find(|child| {
        child.get_name() == token || child.get_all_aliases().any(|alias| alias == token)
    })
}

fn flag_takes_value(current: &clap::Command, root: &clap::Command, token: &str) -> bool {
    let long = token.strip_prefix("--");
    let short = token
        .strip_prefix('-')
        .filter(|value| value.len() == 1)
        .and_then(|value| value.chars().next());

    current
        .get_arguments()
        .chain(root.get_arguments())
        .find(|argument| {
            long.is_some_and(|name| argument.get_long() == Some(name))
                || short.is_some_and(|name| argument.get_short() == Some(name))
        })
        .is_some_and(|argument| argument.get_action().takes_values())
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    #[test]
    fn adapts_success_to_public_contract() {
        let source = json!({
            "data": {"environment": "ote"},
            "metadata": {"system": "gddy"},
            "warnings": ["hidden"],
            "next_actions": [{"command": "gddy env set <env>", "description": "Change env"}]
        })
        .to_string();

        let rendered =
            super::adapt(&source, "gddy env get", true).expect("JSON envelope should adapt");
        let actual: Value = serde_json::from_str(&rendered).expect("valid JSON");

        assert_eq!(
            actual,
            json!({
                "ok": true,
                "command": "gddy env get",
                "result": {"environment": "ote"},
                "next_actions": [{"command": "gddy env set <env>", "description": "Change env"}]
            })
        );
        assert!(rendered.ends_with('\n'));
    }

    #[test]
    fn adapts_failure_to_public_contract() {
        let source = json!({
            "error": {
                "code": "INVALID_ENV",
                "message": "unknown environment",
                "system": "gddy",
                "details": {"env": "bad"}
            }
        })
        .to_string();

        let rendered =
            super::adapt(&source, "gddy env get", false).expect("JSON error envelope should adapt");
        let actual: Value = serde_json::from_str(&rendered).expect("valid JSON");

        assert_eq!(
            actual,
            json!({
                "ok": false,
                "command": "gddy env get",
                "error": {"code": "INVALID_ENV", "message": "unknown environment"},
                "next_actions": []
            })
        );
    }

    #[test]
    fn defaults_missing_next_actions_to_empty_array() {
        let rendered = super::adapt(r#"{"data":null}"#, "gddy env get", true)
            .expect("JSON envelope should adapt");
        let actual: Value = serde_json::from_str(&rendered).expect("valid JSON");
        assert_eq!(actual["next_actions"], json!([]));
    }

    #[test]
    fn leaves_non_json_output_unadapted() {
        assert!(super::adapt("Usage: gddy <COMMAND>\n", "gddy", true).is_none());
    }

    #[test]
    fn leaves_streaming_failures_unadapted() {
        let native = json!({
            "error": {
                "code": "AUTH_REQUIRED",
                "message": "authentication required"
            },
            "next_actions": []
        })
        .to_string();

        assert!(super::adapt(&native, "gddy application deploy", false).is_none());
    }

    fn command_root() -> clap::Command {
        clap::Command::new("gddy")
            .arg(
                clap::Arg::new("debug")
                    .long("debug")
                    .global(true)
                    .num_args(0..=1)
                    .default_missing_value("*"),
            )
            .subcommand(
                clap::Command::new("application").subcommand(
                    clap::Command::new("info").arg(clap::Arg::new("name").required(true)),
                ),
            )
            .subcommand(clap::Command::new("env").subcommand(clap::Command::new("get")))
    }

    fn args(values: &[&str]) -> Vec<std::ffi::OsString> {
        values.iter().map(std::ffi::OsString::from).collect()
    }

    #[test]
    fn command_path_contains_only_recognized_commands() {
        assert_eq!(
            super::command_path(
                &command_root(),
                &args(&["gddy", "application", "info", "secret-app"]),
            ),
            "gddy application info"
        );
    }

    #[test]
    fn command_path_handles_optional_global_flag_before_commands() {
        assert_eq!(
            super::command_path(&command_root(), &args(&["gddy", "--debug", "env", "get"]),),
            "gddy env get"
        );
    }

    #[test]
    fn command_path_handles_optional_global_flag_between_commands() {
        assert_eq!(
            super::command_path(
                &command_root(),
                &args(&["gddy", "application", "--debug", "info", "secret-app"]),
            ),
            "gddy application info"
        );
    }

    #[test]
    fn command_path_survives_invalid_leaf_arguments_without_exposing_values() {
        assert_eq!(
            super::command_path(
                &command_root(),
                &args(&[
                    "gddy",
                    "application",
                    "info",
                    "secret-app",
                    "--invalid",
                    "secret-value",
                ]),
            ),
            "gddy application info"
        );
    }
}
