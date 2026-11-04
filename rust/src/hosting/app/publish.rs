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
                 `hosting deployment publish` does. Every run deploys again. The app must be \
                 attached to a hosting plan first, which can be done by running `hosting \
                 subscription list` then `hosting subscription attach`. Returns a deployment ID; \
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
    async fn publish_and_unpublish_exist_only_while_mhwp_is_on() {
        let off = cli(false);
        let help = run_hosting(&off, &["app", "--help"]).await;
        assert!(!help.contains("publish"), "{help}");
        let missing = run_hosting(&off, &["app", "publish", "--app-id", "MHWP-a1b2c3d4e5"]).await;
        assert!(missing.contains("unknown command \"publish\""), "{missing}");

        let on = cli(true);
        let help = run_hosting(&on, &["app", "--help"]).await;
        assert!(
            help.contains("publish") && help.contains("unpublish"),
            "{help}"
        );
        let help = run_hosting(&on, &["app", "publish", "--help"]).await;
        assert!(help.contains("placeholder page"), "{help}");
        assert!(help.contains("--app-id"), "{help}");
        let help = run_hosting(&on, &["app", "unpublish", "--help"]).await;
        assert!(help.contains("placeholder page"), "{help}");
        // Parses, then stops at auth: no provider is registered in tests.
        let parsed = run_hosting(&on, &["app", "publish", "--app-id", "MHWP-a1b2c3d4e5"]).await;
        assert!(parsed.contains("no provider registered"), "{parsed}");
    }

    #[tokio::test]
    async fn help_text_names_publish_only_while_mhwp_is_on() {
        let off = cli(false);
        let on = cli(true);
        // `hosting deployment publish` is a Node.js command and stays in every mode, so
        // the check is for the WordPress commands themselves.
        let help = run_hosting(&off, &["--help"]).await;
        assert!(
            !help.contains("unpublish") && !help.contains("app publish"),
            "{help}"
        );
        let help = run_hosting(&on, &["--help"]).await;
        assert!(help.contains("publish/unpublish"), "{help}");
        // Polling an operation reads the same whichever products are on.
        for args in [
            &["operation", "--help"][..],
            &["operation", "get", "--help"],
        ] {
            assert_eq!(
                run_hosting(&off, args).await,
                run_hosting(&on, args).await,
                "{args:?}"
            );
        }
    }
}
