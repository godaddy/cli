use cli_engine::{CommandResult, CommandSpec, NextActionParam, RuntimeCommandSpec, Tier};

use crate::hosting::client::ClientError;
use crate::hosting::common::{
    AppIdArgs, HostingDeploymentSummary, client_err, client_err_with_fix, make_client,
};
use crate::next_action::next_action;
use crate::scopes::HOSTING_DEPLOYMENT_EXECUTE as DEPLOY_EXECUTE;

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<AppIdArgs, _, _, _>(
        CommandSpec::from_args::<AppIdArgs>("publish", "Trigger a deployment")
            .with_long(
                "Promote the current PREVIEW build to PUBLISH. Hosting uses a two-stage model: \
                 `hosting source upload` refreshes PREVIEW (or `hosting source github` \
                 if the app is already GitHub-linked in the UI), \
                 and `hosting deployment publish` builds that source and rolls it out to PUBLISH — \
                 which is why there is no --variant flag here. Requires a subscription on the app; \
                 first time only, run `hosting subscription list` then `hosting subscription attach`. \
                 Returns immediately with a deployment ID; poll `hosting deployment get` until \
                 status is COMPLETED or FAILED.",
            )
            .with_system("hosting")
            .with_tier(Tier::Mutate)
            .mutates(true)
            .with_scopes(&[DEPLOY_EXECUTE])
            .with_output_schema::<HostingDeploymentSummary>(),
        |ctx, args: AppIdArgs| async move {
            let app_id = args.app_id;
            let client = make_client(&ctx, &[DEPLOY_EXECUTE]).await?;
            let data = client
                .create_deployment(&app_id)
                .await
                .map_err(publish_err)?;
            let deployment_id = data
                .get("deploymentId")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_owned();
            Ok(CommandResult::new(data).with_next_actions(vec![
                next_action(
                    "hosting deployment get --app-id <app-id> --deployment-id <id>",
                    "Poll until deployment completes",
                )
                .with_param("app-id", NextActionParam::value(app_id))
                .with_param("deployment-id", NextActionParam::value(deployment_id)),
            ]))
        },
    )
}

fn publish_err(e: ClientError) -> cli_engine::CliCoreError {
    match &e {
        ClientError::Http { status, .. } if *status == 422 => client_err_with_fix(
            e,
            "Run: gddy hosting subscription list --hosting-product=WEB_HOSTING",
        ),
        _ => client_err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publish_err_points_422_at_subscription_list() {
        let err = publish_err(ClientError::Http {
            status: 422,
            body: r#"{"message":"This app must be attached to a Web Hosting plan to publish.","details":[{"issue":"WH_PLAN_REQUIRED"}]}"#.to_owned(),
        });
        let envelope = cli_engine::build_error_envelope(&err, "hosting");
        assert_eq!(
            envelope.fix.as_deref(),
            Some("Run: gddy hosting subscription list --hosting-product=WEB_HOSTING")
        );
        assert!(
            envelope
                .error
                .as_ref()
                .is_some_and(|e| e.message.contains("WH_PLAN_REQUIRED")),
            "{envelope:?}"
        );
    }

    #[test]
    fn publish_err_keeps_generic_fix_for_other_client_errors() {
        let err = publish_err(ClientError::Http {
            status: 500,
            body: r#"{"message":"unavailable"}"#.to_owned(),
        });
        let envelope = cli_engine::build_error_envelope(&err, "hosting");
        assert!(
            envelope
                .fix
                .as_deref()
                .is_some_and(|f| f.contains("server-side")),
            "{envelope:?}"
        );
    }
}
