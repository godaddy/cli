use cli_engine::{CommandResult, CommandSpec, NextActionParam, RuntimeCommandSpec, Tier};

use crate::hosting::client::{ClientError, HostingClient};
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
            let client = make_client(&ctx, &[DEPLOY_EXECUTE]).await?;
            deploy(&client, args.app_id).await
        },
    )
}

/// Builds the app's latest imported source and rolls it out to PUBLISH: the
/// body of `hosting deployment publish`, and of `hosting app publish` for a
/// Node.js app.
pub(crate) async fn deploy(
    client: &HostingClient,
    app_id: String,
) -> cli_engine::Result<CommandResult> {
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
    use httpmock::prelude::*;
    use serde_json::json;

    use super::*;

    #[tokio::test]
    async fn deploy_starts_a_deployment_and_points_at_its_poller() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v1/hosting/apps/NODEJS-aBcDeFgHiJ/deployments");
                then.status(202)
                    .json_body(json!({ "deploymentId": "dep-1", "status": "PENDING" }));
            })
            .await;

        let client = HostingClient::new(server.base_url(), "test-token");
        let result = deploy(&client, "NODEJS-aBcDeFgHiJ".to_owned())
            .await
            .expect("deploy");

        mock.assert_async().await;
        assert_eq!(result.data["deploymentId"], "dep-1");
        let actions = &result.metadata.next_actions;
        assert_eq!(actions.len(), 1);
        assert_eq!(
            actions[0].command,
            "gddy hosting deployment get --app-id <app-id> --deployment-id <id>"
        );
        assert_eq!(
            actions[0].params["app-id"].value.as_deref(),
            Some("NODEJS-aBcDeFgHiJ")
        );
        assert_eq!(
            actions[0].params["deployment-id"].value.as_deref(),
            Some("dep-1")
        );
    }

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
