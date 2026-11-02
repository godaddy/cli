use cli_engine::{Cli, CliConfig, Stage};

pub(crate) fn cli(mhwp: bool) -> Cli {
    let mut config = CliConfig::new("gddy", "GoDaddy developer CLI", "gddy")
        .with_min_stage(Stage::Beta)
        // Unregistered provider: commands that pass parsing stop at auth.
        .with_default_auth_provider("godaddy")
        .with_module(crate::hosting::module());
    if mhwp {
        config = config.with_feature_override("hosting-mhwp", Stage::Ga);
    }
    Cli::new(config)
}

/// Runs `gddy hosting <args>` and returns the rendered output.
pub(crate) async fn run_hosting(cli: &Cli, args: &[&str]) -> String {
    let mut argv = vec!["gddy", "hosting"];
    argv.extend_from_slice(args);
    cli.run(argv).await.rendered
}

#[tokio::test]
async fn required_flags_are_marked_in_help_and_enforced() {
    let cli = cli(false);
    let cases: [(&[&str], &str); 3] = [
        (&["app", "list"], "--app-type"),
        (&["app", "create", "--name", "x"], "--app-type"),
        (&["subscription", "list"], "--hosting-product"),
    ];
    for (command, flag) in cases {
        let missing = run_hosting(&cli, command).await;
        assert!(
            missing.contains("required arguments were not provided") && missing.contains(flag),
            "{command:?}: {missing}"
        );

        let mut help_args = command.to_vec();
        help_args.push("--help");
        let help = run_hosting(&cli, &help_args).await;
        // The entry is the flag line plus its description, up to the next flag.
        let entry: Vec<&str> = help
            .lines()
            .skip_while(|line| !line.trim_start().starts_with(flag))
            .enumerate()
            .take_while(|(i, line)| *i == 0 || !line.trim_start().starts_with("-"))
            .map(|(_, line)| line)
            .collect();
        assert!(
            !entry.is_empty(),
            "{command:?}: no entry for {flag}: {help}"
        );
        assert!(
            entry.iter().any(|line| line.contains("(required)")),
            "{command:?}: {help}"
        );
    }
}
