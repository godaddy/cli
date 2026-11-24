//! Shared by `hosting deployment publish`, `get` and `list`: what to tell the
//! customer when a deployment is refused, cannot be read, or fails.

use std::time::Duration;

use cli_engine::{NextAction, NextActionParam};
use serde_json::Value;

use crate::hosting::common::{app_id_arg, printable_link};
use crate::next_action::next_action;

/// The `rel` of the error link that leads to the plan page of an app on Airo
/// App Builder.
pub(super) const UPGRADE_LINK: &str = "upgrade";
pub(super) const PLAN_UPGRADE_REQUIRED: &str = "PLAN_UPGRADE_REQUIRED";
pub(super) const APP_SUSPENDED: &str = "APP_SUSPENDED";

/// How long to wait before trying again: the delay the service asked for, or
/// a few seconds when it named none.
pub(super) fn wait_for(retry_after: Option<Duration>) -> String {
    match retry_after.map(|delay| delay.as_secs()) {
        Some(1) => "Wait 1 second".to_owned(),
        Some(seconds) if seconds > 1 => format!("Wait {seconds} seconds"),
        _ => "Wait a few seconds".to_owned(),
    }
}

/// Where to upgrade the plan of an app on Airo App Builder before `then`: the
/// page the service names, or Airo App Builder itself when it names none. The
/// link ends the text, so nothing after it can be taken as part of it.
pub(super) fn plan_upgrade_text(upgrade_url: Option<String>, then: &str) -> String {
    const NOT_LIVE: &str = "Your Airo App Builder plan does not let this app go live.";
    match upgrade_url {
        Some(url) => format!("{NOT_LIVE} Upgrade the plan, then {then}. Upgrade page: {url}"),
        None => format!("{NOT_LIVE} Upgrade the plan in Airo App Builder, then {then}."),
    }
}

pub(super) fn suspended_text(then: &str) -> String {
    format!(
        "This app is suspended, so it cannot go live. Check the app and your plan in Airo App \
         Builder, then {then}."
    )
}

/// The `501` a Node.js app gets while going live from the command line is not
/// offered for it. Only an app on Airo App Builder is answered so, but its id
/// does not say which product it is on, so the text names where to look.
pub(super) fn not_offered_text(app_id: &str) -> String {
    format!(
        "Going live from the command line is not available for this app yet. If `gddy hosting \
         app get {}` shows `hostingProduct` AIRO, publish the app from Airo App Builder.",
        app_id_arg(app_id)
    )
}

/// What to do about a deployment that FAILED because of the app's Airo App
/// Builder plan: a warning that says why, with the upgrade link of
/// `error.links` when there is a printable one, and the publish to run once
/// that is resolved. `None` for any other deployment.
pub(super) fn plan_failure(deployment: &Value, app_id: &str) -> Option<(String, NextAction)> {
    if deployment["status"] != "FAILED" {
        return None;
    }
    let error = &deployment["error"];
    let has = |code: &str| {
        error["details"]
            .as_array()
            .is_some_and(|details| details.iter().any(|d| d["issue"] == code))
    };
    const THEN: &str = "publish the app again";
    let (warning, description) = if has(PLAN_UPGRADE_REQUIRED) {
        (
            plan_upgrade_text(printable_link(error, UPGRADE_LINK), THEN),
            "Publish again once the plan is upgraded",
        )
    } else if has(APP_SUSPENDED) {
        (
            suspended_text(THEN),
            "Publish again once the app is no longer suspended",
        )
    } else {
        return None;
    };
    let publish = next_action("hosting deployment publish --app-id <app-id>", description)
        .with_param("app-id", NextActionParam::value(app_id));
    Some((warning, publish))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn wait_for_names_the_delay_the_service_asked_for() {
        for (retry_after, expected) in [
            (Some(30), "Wait 30 seconds"),
            (Some(1), "Wait 1 second"),
            (Some(0), "Wait a few seconds"),
            (None, "Wait a few seconds"),
        ] {
            assert_eq!(
                wait_for(retry_after.map(Duration::from_secs)),
                expected,
                "{retry_after:?}"
            );
        }
    }

    fn failed(issue: &str, links: Value) -> Value {
        json!({
            "deploymentId": "dep-1",
            "status": "FAILED",
            "error": {
                "name": "DeploymentFailed",
                "message": "m",
                "details": [{ "issue": issue }],
                "links": links
            },
            "links": []
        })
    }

    #[test]
    fn a_plan_failure_names_the_upgrade_page_of_the_error() {
        let deployment = failed(
            PLAN_UPGRADE_REQUIRED,
            json!([{ "rel": "upgrade", "href": "https://www.example.com/manage-plan?appId=x" }]),
        );
        let (warning, action) = plan_failure(&deployment, "NODEJS-aBcDeFgHiJ").expect("guidance");
        assert_eq!(
            warning,
            "Your Airo App Builder plan does not let this app go live. Upgrade the plan, then \
             publish the app again. Upgrade page: https://www.example.com/manage-plan?appId=x"
        );
        assert_eq!(
            action.command,
            "gddy hosting deployment publish --app-id <app-id>"
        );
        assert_eq!(
            action.params["app-id"].value.as_deref(),
            Some("NODEJS-aBcDeFgHiJ")
        );
    }

    #[test]
    fn a_plan_failure_without_a_printable_link_points_at_airo_app_builder() {
        for links in [
            Value::Null,
            json!([]),
            json!([{ "rel": "upgrade", "href": "http://www.example.com/plans" }]),
            json!("x"),
        ] {
            let (warning, _) =
                plan_failure(&failed(PLAN_UPGRADE_REQUIRED, links.clone()), "NODEJS-x")
                    .expect("guidance");
            assert!(
                warning.contains("Upgrade the plan in Airo App Builder")
                    && !warning.contains("http"),
                "{links}: {warning}"
            );
        }
    }

    #[test]
    fn a_suspended_app_is_told_to_check_it_in_airo_app_builder() {
        let (warning, action) =
            plan_failure(&failed(APP_SUSPENDED, Value::Null), "NODEJS-x").expect("guidance");
        assert!(warning.contains("is suspended"), "{warning}");
        assert!(action.description.contains("no longer suspended"));
    }

    #[test]
    fn other_deployments_get_no_plan_guidance() {
        for deployment in [
            failed("DEPLOY_FAILED", Value::Null),
            failed("ARCHIVE_FAILED", Value::Null),
            json!({ "status": "FAILED" }),
            json!({ "status": "FAILED", "error": null }),
            json!({ "status": "COMPLETED", "error": { "details": [{ "issue": PLAN_UPGRADE_REQUIRED }] } }),
            json!({ "status": "IN_PROGRESS" }),
            Value::Null,
        ] {
            assert!(
                plan_failure(&deployment, "NODEJS-x").is_none(),
                "{deployment}"
            );
        }
    }

    #[test]
    fn the_not_offered_text_names_the_app_only_when_it_has_an_id_shape() {
        assert!(
            not_offered_text("NODEJS-aBcDeFgHiJ")
                .contains("`gddy hosting app get --app-id NODEJS-aBcDeFgHiJ`")
        );
        assert!(
            not_offered_text("NODEJS-x;rm -rf ~")
                .contains("`gddy hosting app get --app-id <app-id>`")
        );
    }
}
