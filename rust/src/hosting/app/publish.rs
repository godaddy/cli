use cli_engine::{CommandSpec, RuntimeCommandSpec, Tier};

use crate::hosting::app::common::{PublishArgs, publish};
use crate::hosting::common::{HostingAppPublish, make_client};
use crate::scopes::HOSTING_DEPLOYMENT_EXECUTE as DEPLOY_EXECUTE;

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<PublishArgs, _, _, _>(
        CommandSpec::from_args::<PublishArgs>("publish", "Take an app live")
            .with_long(
                "Take an application live at its publish URL. What that takes depends on the \
                 product, which the app ID's prefix names:\n\
                 \n\
                 • Node.js (NODEJS-…): builds the latest source from `hosting source upload` or \
                 `hosting source github` and deploys it to the PUBLISH environment, exactly as \
                 `hosting deployment publish` does. Every run deploys again. An app on a Web \
                 Hosting plan must have that plan attached first, which can be done by running \
                 `hosting subscription list` then `hosting subscription attach`. An app on Airo \
                 App Builder has no plan to attach: it goes live when your Airo App Builder plan \
                 allows it, and the error names the page to upgrade the plan on when it does \
                 not. Returns a deployment ID; \
                 poll `hosting deployment get` until the status is COMPLETED or FAILED.\n\
                 \n\
                 • WordPress (MHWP-…): switches the site from its placeholder page to its live \
                 content; there is no build step. Going live needs a plan with a free \
                 published-site slot; the free plan has none. Returns an operation ID; poll \
                 `hosting operation get` until status is COMPLETED or FAILED. `hosting app \
                 unpublish` takes the site back to its placeholder page.",
            )
            .with_system("hosting")
            .with_tier(Tier::Mutate)
            .mutates(true)
            .with_scopes(&[DEPLOY_EXECUTE])
            .with_output_schema::<HostingAppPublish>(),
        |ctx, args: PublishArgs| async move {
            let client = make_client(&ctx, &[DEPLOY_EXECUTE]).await?;
            publish(&client, args.app).await
        },
    )
}

#[cfg(test)]
mod tests {
    use crate::hosting::test_support::{cli, run_hosting};

    /// Taking a site live is the WordPress equivalent of publishing a Node.js
    /// deployment, so both commands step up to the deployment-execute scope the
    /// API now requires on `PUT /apps/{appId}/publish-state` — not the
    /// application-update scope, which no longer opens that route.
    #[test]
    fn both_commands_declare_the_deployment_execute_scope() {
        for spec in [
            super::command().spec,
            super::super::unpublish::command().spec,
        ] {
            let scopes = spec.metadata().scopes;
            assert_eq!(scopes, vec!["hosting.deployment:execute".to_owned()]);
            assert!(spec.mutates);
        }
    }

    #[tokio::test]
    async fn publish_and_unpublish_are_registered() {
        let cli = cli();
        let help = run_hosting(&cli, &["app", "--help"]).await;
        assert!(help.contains("Take an app live"), "{help}");
        assert!(help.contains("Take a WordPress site offline"), "{help}");
        for command in ["publish", "unpublish"] {
            let help = run_hosting(&cli, &["app", command, "--help"]).await;
            assert!(help.contains("placeholder page"), "{command}: {help}");
            assert!(help.contains("--app-id"), "{command}: {help}");
            // Parses, then stops at auth: no provider is registered in tests.
            let parsed = run_hosting(&cli, &["app", command, "--app-id", "MHWP-a1b2c3d4e5"]).await;
            assert!(
                parsed.contains("no provider registered"),
                "{command}: {parsed}"
            );
        }
    }

    #[tokio::test]
    async fn the_hosting_help_names_publish_and_unpublish() {
        let help = run_hosting(&cli(), &["--help"]).await;
        assert!(help.contains("publish/unpublish"), "{help}");
        assert!(help.contains("First-time setup (Node.js)"), "{help}");
        assert!(help.contains("gddy guide hosting-wordpress"), "{help}");
    }
}
