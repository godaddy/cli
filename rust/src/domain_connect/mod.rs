//! `gddy domain-connect` — create, check and submit Domain Connect templates.

use cli_engine::{GroupSpec, Module, RuntimeGroupSpec, Stage};

mod template;

const FLAG: &str = "domain-connect";
const STAGE: Stage = Stage::Experimental;

pub fn module() -> Module {
    Module::new("Domain Connect", |ctx| {
        // Registered here, not on the module, so the guide is listed only
        // when the commands it describes are.
        if ctx.middleware().flag_policy.visible(Some(FLAG), STAGE) {
            ctx.add_guides_from_markdown([(
                "domain-connect.md",
                include_bytes!("guides/domain-connect.md").as_slice(),
            )]);
        }
        RuntimeGroupSpec::new(
            GroupSpec::new("domain-connect", "Publish Domain Connect templates").with_long(
                "Publish a Domain Connect template, so customers can connect a domain to \
                 your service in one step.\n\
                 \n\
                 • template init     — create a template file\n\
                 • template validate — check the file; changes nothing\n\
                 • template submit   — publish the template for review\n\
                 \n\
                 Run `gddy guide domain-connect` for the full walkthrough.",
            ),
        )
        .with_group(template::group())
    })
    .with_feature_flag(FLAG, STAGE)
}

#[cfg(test)]
mod tests {
    use cli_engine::{Cli, CliConfig, Stage};

    fn cli(min_stage: Stage) -> Cli {
        Cli::new(
            CliConfig::new("gddy", "GoDaddy developer CLI", "gddy")
                .with_min_stage(min_stage)
                .with_modules(crate::all_modules()),
        )
    }

    #[tokio::test]
    async fn the_commands_and_the_guide_are_hidden_until_the_stage_allows_them() {
        let hidden = cli(Stage::Ga);
        let help = hidden.run(["gddy", "domain-connect", "--help"]).await;
        assert_ne!(help.exit_code, 0, "{}", help.rendered);
        let guides = hidden.run(["gddy", "guide"]).await;
        assert!(
            !guides.rendered.contains("domain-connect"),
            "{}",
            guides.rendered
        );

        let revealed = cli(Stage::Experimental);
        for command in ["init", "validate", "submit"] {
            let help = revealed
                .run(["gddy", "domain-connect", "template", command, "--help"])
                .await;
            assert_eq!(help.exit_code, 0, "{command}: {}", help.rendered);
        }
        let plural = revealed
            .run(["gddy", "domain-connect", "templates", "validate", "--help"])
            .await;
        assert_eq!(plural.exit_code, 0, "{}", plural.rendered);
        let guide = revealed.run(["gddy", "guide", "domain-connect"]).await;
        assert_eq!(guide.exit_code, 0, "{}", guide.rendered);
        assert!(
            guide.rendered.contains("template submit"),
            "{}",
            guide.rendered
        );
    }

    #[tokio::test]
    async fn init_writes_nothing_and_needs_no_sign_in_under_dry_run() {
        let output = cli(Stage::Experimental)
            .run([
                "gddy",
                "domain-connect",
                "template",
                "init",
                "--non-interactive",
                "--provider-id",
                "dry-run-check.example",
                "--provider-name",
                "Acme",
                "--service-id",
                "svc",
                "--service-name",
                "Svc",
                "--add-a",
                "value=192.0.2.1",
                "--dry-run",
                "--output",
                "json",
            ])
            .await;

        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let payload: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("json envelope");
        assert_eq!(payload["data"]["action"], "would create");
        assert_eq!(
            payload["data"]["template"]["records"][0]["pointsTo"],
            "192.0.2.1"
        );
        assert!(!std::path::Path::new("dry-run-check.example.svc.json").exists());
    }

    fn scripted_cli(prompter: std::sync::Arc<cli_engine::prompt::ScriptedPrompter>) -> Cli {
        Cli::new(
            CliConfig::new("gddy", "GoDaddy developer CLI", "gddy")
                .with_min_stage(Stage::Experimental)
                .with_prompter(prompter)
                .with_modules(crate::all_modules()),
        )
    }

    fn init_args<'a>(extra: &[&'a str]) -> Vec<&'a str> {
        let mut args = vec![
            "gddy",
            "domain-connect",
            "template",
            "init",
            "--dry-run",
            "--output",
            "json",
        ];
        args.extend_from_slice(extra);
        args
    }

    #[tokio::test]
    async fn init_asks_only_for_what_the_flags_leave_out() {
        use cli_engine::prompt::{Answer, ScriptedPrompter};

        // Provider ID and service ID come from flags; the rest is asked,
        // then one A record, then no more.
        let prompter = std::sync::Arc::new(ScriptedPrompter::new([
            Answer::text("Acme Corp"),
            Answer::text("Acme Email"),
            Answer::text(""),
            Answer::Confirm(true),
            Answer::Select(0),
            Answer::text("www"),
            Answer::text("192.0.2.1"),
            Answer::text(""),
            Answer::text(""),
            Answer::text(""),
            Answer::Confirm(false),
        ]));
        let output = scripted_cli(std::sync::Arc::clone(&prompter))
            .run(init_args(&[
                "--interactive",
                "--provider-id",
                "prompted.example",
                "--service-id",
                "svc",
            ]))
            .await;

        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let payload: serde_json::Value = serde_json::from_str(&output.rendered).expect("json");
        assert_eq!(payload["data"]["providerId"], "prompted.example");
        assert_eq!(payload["data"]["template"]["providerName"], "Acme Corp");
        assert_eq!(
            payload["data"]["template"]["records"][0]["pointsTo"],
            "192.0.2.1"
        );
        assert_eq!(prompter.remaining(), 0);
        let asked = prompter.transcript().join("\n");
        assert!(!asked.contains("Provider ID"), "{asked}");
        assert!(!asked.contains("Service ID"), "{asked}");
    }

    #[tokio::test]
    async fn init_without_prompts_and_without_flags_fails_with_a_fix() {
        use cli_engine::prompt::{Answer, ScriptedPrompter};

        let prompter = std::sync::Arc::new(ScriptedPrompter::new([Answer::text("unused")]));
        let output = scripted_cli(std::sync::Arc::clone(&prompter))
            .run(init_args(&["--non-interactive"]))
            .await;

        assert_ne!(output.exit_code, 0, "{}", output.rendered);
        assert!(
            output.rendered.contains("--provider-id"),
            "{}",
            output.rendered
        );
        assert_eq!(prompter.remaining(), 1, "no prompt may be asked");
    }

    #[tokio::test]
    async fn cancelling_init_exits_130_and_writes_nothing() {
        use cli_engine::prompt::{Answer, ScriptedPrompter};

        let prompter = std::sync::Arc::new(ScriptedPrompter::new([Answer::Cancel]));
        let output = scripted_cli(prompter)
            .run(init_args(&["--interactive"]))
            .await;

        assert_eq!(output.exit_code, 130, "{}", output.rendered);
    }

    #[tokio::test]
    async fn init_no_longer_has_its_own_no_interactive_flag() {
        let help = cli(Stage::Experimental)
            .run(["gddy", "domain-connect", "template", "init", "--help"])
            .await;
        assert_eq!(help.exit_code, 0, "{}", help.rendered);
        assert!(
            !help.rendered.contains("--no-interactive"),
            "{}",
            help.rendered
        );
        assert!(
            help.rendered.contains("Interactive by default"),
            "{}",
            help.rendered
        );
    }
}
