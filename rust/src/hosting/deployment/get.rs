use cli_engine::{
    CommandResult, CommandSpec, NextAction, NextActionParam, RuntimeCommandSpec, Tier,
};
use serde_json::Value;

use super::common::{not_offered_text, plan_failure, wait_for};
use crate::hosting::client::ClientError;
use crate::hosting::common::{
    HostingAppType, HostingDeploymentSummary, client_err, client_err_with_fix, error_issues,
    make_client,
};
use crate::next_action::next_action;
use crate::scopes::HOSTING_APPLICATION_READ as APP_READ;

#[derive(Debug, Clone, clap::Args)]
struct DeploymentGetArgs {
    /// Application ID.
    #[arg(long = "app-id", value_name = "APP_ID")]
    app_id: String,

    /// Deployment ID.
    #[arg(
        long = "deployment-id",
        value_name = "DEPLOYMENT_ID",
        allow_hyphen_values = true
    )]
    deployment_id: String,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<DeploymentGetArgs, _, _, _>(
        CommandSpec::from_args::<DeploymentGetArgs>("get", "Get a deployment")
            .with_long(
                "Get the current status and details of a specific deployment. Run it again \
                 until status is COMPLETED or FAILED. A FAILED deployment's `error` says why; \
                 when an app on Airo App Builder failed because of its plan, the output also \
                 says where to upgrade the plan.",
            )
            .with_system("hosting")
            .with_tier(Tier::Read)
            .with_scopes(&[APP_READ])
            .with_output_schema::<HostingDeploymentSummary>(),
        |ctx, args: DeploymentGetArgs| async move {
            let app_id = args.app_id.clone();
            let deployment_id = args.deployment_id;
            let client = make_client(&ctx, &[APP_READ]).await?;
            let data = client
                .get_deployment(&app_id, &deployment_id)
                .await
                .map_err(|e| poll_err(e, &app_id))?;
            Ok(result(data, &app_id))
        },
    )
}

/// The deployment, with what to do next: see the app once it is live, or
/// resolve the plan of an app whose deployment failed because of it.
fn result(data: Value, app_id: &str) -> CommandResult {
    if let Some((warning, publish)) = plan_failure(&data, app_id) {
        return CommandResult::new(data)
            .with_warning(warning)
            .with_next_actions(vec![publish]);
    }
    let next_actions = next_actions(&data, app_id);
    CommandResult::new(data).with_next_actions(next_actions)
}

/// A read of a deployment that the service could not answer. A `503` there
/// leaves the deployment as it was, so the fix says to check it again.
fn poll_err(e: ClientError, app_id: &str) -> cli_engine::CliCoreError {
    let ClientError::Http {
        status,
        retry_after,
        ..
    } = &e
    else {
        return client_err(e);
    };
    let fix = match *status {
        503 if error_issues(&e).iter().any(|i| i == "UPSTREAM_UNAVAILABLE") => format!(
            "The deployment's status could not be read right now; this does not mean it \
             failed. {} and run this command again to check it.",
            wait_for(*retry_after)
        ),
        501 if HostingAppType::of_id(app_id) == Some(HostingAppType::Nodejs) => {
            not_offered_text(app_id)
        }
        _ => return client_err(e),
    };
    client_err_with_fix(e, fix)
}

fn next_actions(data: &Value, app_id: &str) -> Vec<NextAction> {
    if data["status"] != "COMPLETED" {
        return Vec::new();
    }
    vec![
        next_action(
            "hosting app get --app-id <app-id>",
            "See the URL for each environment",
        )
        .with_param("app-id", NextActionParam::value(app_id)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::{Args, FromArgMatches};
    use serde_json::json;

    #[test]
    fn completed_deployment_suggests_app_get() {
        let actions = next_actions(&json!({ "status": "COMPLETED" }), "NODEJS-1");
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].command, "gddy hosting app get --app-id <app-id>");
        assert_eq!(
            actions[0].params["app-id"].value.as_deref(),
            Some("NODEJS-1")
        );
    }

    #[test]
    fn unfinished_or_failed_deployment_suggests_nothing() {
        for status in ["IN_PROGRESS", "FAILED"] {
            assert!(next_actions(&json!({ "status": status }), "NODEJS-1").is_empty());
        }
        assert!(next_actions(&json!({}), "NODEJS-1").is_empty());
    }

    /// The deployment the service answers a poll with when the app's Airo App
    /// Builder plan stopped allowing it to go live after the publish was
    /// accepted: the link is on the deployment's `error`, not at the top.
    fn failed_on_plan() -> Value {
        json!({
            "deploymentId": "dep-1",
            "status": "FAILED",
            "gitHash": "abc123",
            "createdAt": "2026-11-24T10:00:00Z",
            "updatedAt": "2026-11-24T10:01:00Z",
            "error": {
                "name": "DeploymentFailed",
                "correlationId": "c1",
                "message": "This application needs a paid plan to publish.",
                "details": [{ "issue": "PLAN_UPGRADE_REQUIRED" }],
                "links": [{
                    "rel": "upgrade",
                    "href": "https://airo-builder.godaddy.com/manage-plan?action=showPaywall&appId=aBcDeFgHiJ"
                }]
            },
            "links": [
                { "rel": "self", "href": "/v1/hosting/apps/NODEJS-aBcDeFgHiJ/deployments/dep-1" }
            ]
        })
    }

    #[test]
    fn a_plan_failure_after_the_publish_says_where_to_upgrade() {
        let result = result(failed_on_plan(), "NODEJS-aBcDeFgHiJ");
        assert_eq!(
            result.metadata.warnings,
            [
                "Your Airo App Builder plan does not let this app go live. Upgrade the plan, \
                 then publish the app again. Upgrade page: \
                 https://airo-builder.godaddy.com/manage-plan?action=showPaywall&appId=aBcDeFgHiJ"
            ]
        );
        let actions = &result.metadata.next_actions;
        assert_eq!(actions.len(), 1);
        assert_eq!(
            actions[0].command,
            "gddy hosting deployment publish --app-id <app-id>"
        );
        assert_eq!(
            actions[0].params["app-id"].value.as_deref(),
            Some("NODEJS-aBcDeFgHiJ")
        );
        // The deployment itself is shown as the service sent it.
        assert_eq!(result.data, failed_on_plan());
    }

    /// What users see: the deployment's error and logs, the warning, and the
    /// publish to run once the plan is upgraded, in human and JSON output.
    #[test]
    fn a_plan_failure_reaches_the_rendered_output() {
        let mut deployment = failed_on_plan();
        deployment["logs"] = json!(["Archiving source", "Refused by plan"]);
        let result = result(deployment, "NODEJS-aBcDeFgHiJ");
        let mut envelope = cli_engine::Envelope::success(result.data, "hosting");
        envelope.warnings = result.metadata.warnings;
        envelope.next_actions = result.metadata.next_actions;

        let human = cli_engine::render_human(&envelope);
        assert!(human.contains("PLAN_UPGRADE_REQUIRED"), "{human}");
        assert!(human.contains("Refused by plan"), "{human}");
        assert!(
            human.contains(
                "Upgrade page: \
                 https://airo-builder.godaddy.com/manage-plan?action=showPaywall&appId=aBcDeFgHiJ\n"
            ),
            "{human}"
        );
        assert!(
            human.contains("gddy hosting deployment publish --app-id NODEJS-aBcDeFgHiJ"),
            "{human}"
        );

        let json: Value = serde_json::to_value(&envelope).expect("envelope");
        assert_eq!(
            json["data"]["error"]["details"][0]["issue"],
            "PLAN_UPGRADE_REQUIRED"
        );
        assert!(
            json["warnings"][0]
                .as_str()
                .is_some_and(|w| w.ends_with("appId=aBcDeFgHiJ")),
            "{json}"
        );
    }

    #[test]
    fn a_failure_for_another_reason_gets_no_plan_guidance() {
        let mut deployment = failed_on_plan();
        deployment["error"] = json!({
            "name": "DeploymentFailed",
            "message": "Deploy step failed.",
            "details": [{ "issue": "DEPLOY_FAILED" }]
        });
        let result = result(deployment, "NODEJS-aBcDeFgHiJ");
        assert!(result.metadata.warnings.is_empty());
        assert!(result.metadata.next_actions.is_empty());
    }

    fn poll_fix(
        status: u16,
        issue: &str,
        retry_after: Option<u64>,
        app_id: &str,
    ) -> Option<String> {
        let err = poll_err(
            ClientError::Http {
                status,
                body: format!(
                    r#"{{"message":"Cannot fetch deployment","details":[{{"issue":"{issue}"}}]}}"#
                ),
                retry_after: retry_after.map(std::time::Duration::from_secs),
            },
            app_id,
        );
        cli_engine::build_error_envelope(&err, "hosting").fix
    }

    #[test]
    fn a_503_on_a_poll_says_to_check_again() {
        assert_eq!(
            poll_fix(503, "UPSTREAM_UNAVAILABLE", Some(30), "NODEJS-aBcDeFgHiJ").as_deref(),
            Some(
                "The deployment's status could not be read right now; this does not mean it \
                 failed. Wait 30 seconds and run this command again to check it."
            )
        );
        assert!(
            poll_fix(503, "UPSTREAM_UNAVAILABLE", None, "NODEJS-aBcDeFgHiJ")
                .is_some_and(|f| f.contains("Wait a few seconds")),
        );
        // A 503 without the code keeps the server-side fix.
        assert!(
            poll_fix(503, "OTHER", None, "NODEJS-aBcDeFgHiJ")
                .is_some_and(|f| f.contains("server-side")),
        );
    }

    #[test]
    fn a_501_on_a_poll_gets_its_own_fix() {
        assert_eq!(
            poll_fix(501, "NOT_APPLICABLE", None, "NODEJS-aBcDeFgHiJ").as_deref(),
            Some(
                "Going live from the command line is not available for this app yet. If \
                 `gddy hosting app get --app-id NODEJS-aBcDeFgHiJ` shows `hostingProduct` AIRO, \
                 publish the app from Airo App Builder."
            )
        );
        // A WordPress id has no deployments and keeps the shared fix.
        assert!(
            poll_fix(501, "NOT_APPLICABLE", None, "MHWP-a1b2c3d4e5")
                .is_some_and(|f| f.contains("not supported for this app type")),
        );
    }

    #[test]
    fn deployment_id_starting_with_hyphen_is_parsed_as_a_value() {
        let cmd = DeploymentGetArgs::augment_args(clap::Command::new("get"));
        let matches = cmd
            .try_get_matches_from([
                "get",
                "--app-id",
                "NODEJS-rf2dwbzqux",
                "--deployment-id",
                "-yfNpdQN6X",
            ])
            .expect("hyphen-prefixed deployment ID should not be mistaken for a flag");
        let args =
            DeploymentGetArgs::from_arg_matches(&matches).expect("args should parse into struct");

        assert_eq!(args.deployment_id, "-yfNpdQN6X");
    }
}
