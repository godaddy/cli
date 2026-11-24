use cli_engine::{CommandResult, CommandSpec, NextActionParam, RuntimeCommandSpec, Tier};

use crate::hosting::client::{ClientError, HostingClient};
use crate::hosting::common::{
    AppIdArgs, HostingAppType, HostingDeploymentSummary, client_err, client_err_with_fix,
    error_issues, error_link, make_client,
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
                 which is why there is no --variant flag here. An app on a Web Hosting plan \
                 needs that plan attached; first time only, run `hosting subscription list` then \
                 `hosting subscription attach`. An app on Airo App Builder has no plan to attach: \
                 it goes live when your Airo App Builder plan allows it, and the error names the \
                 page to upgrade the plan on when it does not. `hosting app get` shows which one \
                 an app is on as `hostingProduct`. Returns immediately with a deployment ID; \
                 poll `hosting deployment get` until status is COMPLETED or FAILED.",
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
        .map_err(|e| publish_err(e, &app_id))?;
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

/// The `rel` of the error link that leads to the plan page of an app on Airo
/// App Builder.
const UPGRADE_LINK: &str = "upgrade";

const ATTACH_A_PLAN: &str = "Run: gddy hosting subscription list --hosting-product=WEB_HOSTING";
const APP_SUSPENDED: &str = "This app is suspended, so it cannot go live. Check the app and your \
     plan in Airo App Builder, then run this command again.";
// The service sends this code when it cannot check the plan, and for other
// short outages, so the text has to be true for either.
const TRY_AGAIN_SOON: &str =
    "The service is temporarily unavailable. Wait a few seconds and run the command again.";
const NOT_OFFERED_YET: &str = "Going live from the command line is not available for this app \
     yet. Publish it from Airo App Builder.";

/// The next step for a refused deployment. An app on a Web Hosting plan is
/// sent to attach one. An app on Airo App Builder has nothing to attach, so
/// its errors point at its Airo App Builder plan instead.
fn publish_err(e: ClientError, app_id: &str) -> cli_engine::CliCoreError {
    let ClientError::Http { status, .. } = &e else {
        return client_err(e);
    };
    let issues = error_issues(&e);
    let has = |issue: &str| issues.iter().any(|i| i == issue);
    let fix = match *status {
        422 if has("PLAN_UPGRADE_REQUIRED") => plan_upgrade_fix(error_link(&e, UPGRADE_LINK)),
        422 if has("APP_SUSPENDED") => APP_SUSPENDED.to_owned(),
        422 => ATTACH_A_PLAN.to_owned(),
        // Other products answer 501 too, and keep the shared fix for it.
        501 if HostingAppType::of_id(app_id) == Some(HostingAppType::Nodejs) => {
            NOT_OFFERED_YET.to_owned()
        }
        503 if has("UPSTREAM_UNAVAILABLE") => TRY_AGAIN_SOON.to_owned(),
        _ => return client_err(e),
    };
    client_err_with_fix(e, fix)
}

/// Where to upgrade the plan of an app on Airo App Builder: the page the
/// service names, or Airo App Builder itself when it names none.
fn plan_upgrade_fix(upgrade_url: Option<String>) -> String {
    let place = upgrade_url.map_or_else(
        || "in Airo App Builder".to_owned(),
        |url| format!("at {url}"),
    );
    format!(
        "Your Airo App Builder plan does not let this app go live. Upgrade the plan {place}, \
         then run this command again."
    )
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

    const APP_ID: &str = "NODEJS-aBcDeFgHiJ";

    fn fix_for(status: u16, body: &str, app_id: &str) -> Option<String> {
        let err = publish_err(
            ClientError::Http {
                status,
                body: body.to_owned(),
            },
            app_id,
        );
        cli_engine::build_error_envelope(&err, "hosting").fix
    }

    fn issue(issue: &str) -> String {
        format!(r#"{{"message":"m","details":[{{"issue":"{issue}"}}]}}"#)
    }

    #[test]
    fn a_plan_that_cannot_publish_points_at_its_upgrade_page() {
        // The contract's example body for this error.
        let body = r#"{
            "name": "UnprocessableEntity",
            "correlationId": "6f1c2a9e-1b7d-4c55-9f0e-3c1d2b4a5e6f",
            "message": "This application needs a paid plan to publish.",
            "details": [{
                "issue": "PLAN_UPGRADE_REQUIRED",
                "description": "Plan 'free' allows 0 published apps (0 used)."
            }],
            "links": [{ "rel": "upgrade", "href": "https://www.example.com/airo/plans/NODEJS-aBcDeFgHiJ" }]
        }"#;
        let err = publish_err(
            ClientError::Http {
                status: 422,
                body: body.to_owned(),
            },
            APP_ID,
        );
        let envelope = cli_engine::build_error_envelope(&err, "hosting");
        assert_eq!(
            envelope.fix.as_deref(),
            Some(
                "Your Airo App Builder plan does not let this app go live. Upgrade the plan at \
                 https://www.example.com/airo/plans/NODEJS-aBcDeFgHiJ, then run this command again."
            )
        );
        assert_eq!(
            envelope.error.as_ref().map(|e| e.message.as_str()),
            Some(
                "HTTP error 422: This application needs a paid plan to publish. \
                 (PLAN_UPGRADE_REQUIRED)"
            )
        );
    }

    #[test]
    fn a_plan_error_without_a_usable_link_still_says_where_to_upgrade() {
        for links in [
            "",
            r#","links":[]"#,
            r#","links":[{"rel":"upgrade","href":"http://example.com/plans"}]"#,
            r#","links":"x""#,
        ] {
            let body = format!(
                r#"{{"message":"m","details":[{{"issue":"PLAN_UPGRADE_REQUIRED"}}]{links}}}"#
            );
            let fix = fix_for(422, &body, APP_ID);
            assert_eq!(
                fix.as_deref(),
                Some(
                    "Your Airo App Builder plan does not let this app go live. Upgrade the plan \
                     in Airo App Builder, then run this command again."
                ),
                "{links}"
            );
        }
    }

    #[test]
    fn an_airo_app_is_never_sent_to_attach_a_plan() {
        for (status, issue_code, expected) in [
            (422, "PLAN_UPGRADE_REQUIRED", "Upgrade the plan"),
            (422, "APP_SUSPENDED", "is suspended"),
            (501, "NOT_APPLICABLE", "Publish it from Airo App Builder"),
            (503, "UPSTREAM_UNAVAILABLE", "Wait a few seconds"),
        ] {
            let fix = fix_for(status, &issue(issue_code), APP_ID);
            assert!(
                fix.as_deref()
                    .is_some_and(|f| f.contains(expected) && !f.contains("subscription")),
                "{issue_code}: {fix:?}"
            );
        }
    }

    #[test]
    fn other_refusals_keep_their_fix() {
        // An app on a Web Hosting plan is still sent to attach one.
        for issue_code in [
            "WH_PLAN_REQUIRED",
            "PUBLISH_LIMIT_EXCEEDED",
            "COMPAT_BLOCKED",
        ] {
            let fix = fix_for(422, &issue(issue_code), APP_ID);
            assert_eq!(fix.as_deref(), Some(ATTACH_A_PLAN), "{issue_code}");
        }
        // A product without deployments keeps the shared "not supported" fix.
        let fix = fix_for(501, &issue("NOT_APPLICABLE"), "MHWP-a1b2c3d4e5");
        assert!(
            fix.as_deref()
                .is_some_and(|f| f.contains("not supported for this app type")),
            "{fix:?}"
        );
        // A 503 for another reason keeps the server-side fix.
        let fix = fix_for(503, r#"{"message":"unavailable"}"#, APP_ID);
        assert!(
            fix.as_deref().is_some_and(|f| f.contains("server-side")),
            "{fix:?}"
        );
    }

    #[test]
    fn publish_err_points_422_at_subscription_list() {
        let err = publish_err(
            ClientError::Http {
                status: 422,
                body: r#"{"message":"This app must be attached to a Web Hosting plan to publish.","details":[{"issue":"WH_PLAN_REQUIRED"}]}"#.to_owned(),
            },
            APP_ID,
        );
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
        let err = publish_err(
            ClientError::Http {
                status: 500,
                body: r#"{"message":"unavailable"}"#.to_owned(),
            },
            APP_ID,
        );
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
