use cli_engine::Cli;

pub(crate) struct ParsedAuthLoginArgs {
    pub(crate) accept_agreements: bool,
    pub(crate) output_format: String,
    pub(crate) engine_args: Vec<String>,
}

pub(crate) fn parse(
    cli: &Cli,
    args: &[String],
    default_format: &str,
) -> Option<ParsedAuthLoginArgs> {
    let path_args = args
        .iter()
        .filter(|arg| arg.as_str() != "--accept-agreements")
        .cloned()
        .collect::<Vec<_>>();
    let bool_flags = cli_engine::derive_bool_flags(cli.root_command());
    let value_flags = cli_engine::derive_value_flags(cli.root_command());
    let path = cli_engine::extract_command_path(&path_args, &bool_flags, &value_flags);

    if path != "auth:login"
        || args
            .iter()
            .any(|arg| matches!(arg.as_str(), "--help" | "-h" | "--version" | "-V"))
    {
        return None;
    }

    let output_format = cli_engine::extract_output_format(args, default_format);
    let accept_agreements = args.iter().any(|arg| arg == "--accept-agreements");
    let mut engine_args = Vec::with_capacity(args.len() + 2);
    let mut index = 0;

    while index < args.len() {
        match args[index].as_str() {
            "--accept-agreements" | "--json" | "--toon" | "--human" => {
                index += 1;
            }
            "--output" | "-o" => {
                index += 1;
                if index < args.len() {
                    index += 1;
                }
            }
            arg if arg.starts_with("--output=") => {
                index += 1;
            }
            _ => {
                engine_args.push(args[index].clone());
                index += 1;
            }
        }
    }

    engine_args.extend(["--output".to_owned(), "json".to_owned()]);

    Some(ParsedAuthLoginArgs {
        accept_agreements,
        output_format,
        engine_args,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use cli_engine::{Cli, CliConfig};

    use super::parse;
    use crate::auth::GoDaddyAuthProvider;

    fn argv(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    fn test_cli() -> Cli {
        Cli::new(
            CliConfig::new("gddy", "test", "gddy")
                .with_default_auth_provider("godaddy")
                .with_auth_provider(Arc::new(GoDaddyAuthProvider::new()))
                .with_register_flags(Arc::new(|command| {
                    command.arg(
                        clap::Arg::new("env")
                            .long("env")
                            .global(true)
                            .value_name("ENV"),
                    )
                })),
        )
    }

    #[test]
    fn detects_auth_login_with_global_flags_anywhere() {
        let cli = test_cli();
        assert!(
            parse(
                &cli,
                &argv(&["gddy", "--env", "ote", "auth", "login", "--human"]),
                "json"
            )
            .is_some()
        );
        assert!(
            parse(
                &cli,
                &argv(&["gddy", "auth", "--env", "ote", "login", "--output", "toon"]),
                "json"
            )
            .is_some()
        );
    }

    #[test]
    fn strips_acceptance_and_forces_internal_json() {
        let parsed = parse(
            &test_cli(),
            &argv(&[
                "gddy",
                "auth",
                "login",
                "--accept-agreements",
                "--human",
                "--scope",
                "domains:read",
            ]),
            "json",
        )
        .expect("auth login");

        assert!(parsed.accept_agreements);
        assert_eq!(parsed.output_format, "human");
        assert!(
            !parsed
                .engine_args
                .iter()
                .any(|arg| { matches!(arg.as_str(), "--accept-agreements" | "--human") })
        );
        assert!(
            parsed
                .engine_args
                .ends_with(&["--output".to_owned(), "json".to_owned()])
        );
        assert!(
            parsed
                .engine_args
                .windows(2)
                .any(|pair| { pair == ["--scope".to_owned(), "domains:read".to_owned()] })
        );
    }

    #[test]
    fn preserves_selected_output_format_and_repeated_scopes() {
        let parsed = parse(
            &test_cli(),
            &argv(&[
                "gddy",
                "auth",
                "login",
                "--output=toon",
                "--scope",
                "domains:read",
                "--scope",
                "domains:write",
            ]),
            "human",
        )
        .expect("auth login");

        assert_eq!(parsed.output_format, "toon");
        assert_eq!(
            parsed
                .engine_args
                .windows(2)
                .filter(|pair| pair[0] == "--scope")
                .count(),
            2
        );
        assert!(!parsed.engine_args.iter().any(|arg| arg == "--output=toon"));
        assert!(
            parsed
                .engine_args
                .ends_with(&["--output".to_owned(), "json".to_owned()])
        );
    }

    #[test]
    fn bypasses_status_help_version_and_other_commands() {
        let cli = test_cli();
        for values in [
            &["gddy", "auth", "status"][..],
            &["gddy", "auth", "login", "--help"][..],
            &["gddy", "auth", "login", "-h"][..],
            &["gddy", "--version"][..],
            &["gddy", "-V"][..],
            &["gddy", "domain", "list"][..],
            &["gddy", "domain", "list", "--accept-agreements"][..],
        ] {
            assert!(parse(&cli, &argv(values), "json").is_none(), "{values:?}");
        }
    }
}
