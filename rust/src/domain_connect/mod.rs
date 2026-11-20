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
                "--no-interactive",
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
}
