//! Shared body of `hosting app publish` and `hosting app unpublish`.

use cli_engine::{CommandResult, NextActionParam};

use crate::hosting::client::ClientError;
use crate::hosting::common::{
    MHWP_ID_PREFIX, client_err, client_err_with_fix, error_issues, make_client, strip_id_prefix,
};
use crate::next_action::{next_action, required_value};
use crate::scopes::HOSTING_DEPLOYMENT_EXECUTE as DEPLOY_EXECUTE;

const NODEJS_ID_PREFIX: &str = "NODEJS-";
const NODEJS_PUBLISH_FIX: &str = "Only WordPress (MHWP) apps have a publish state. For a Node.js \
     app run: gddy hosting deployment publish --app-id <app-id>";
const NODEJS_UNPUBLISH_FIX: &str = "Only WordPress (MHWP) apps have a publish state.";

/// `--app-id` for `publish` and `unpublish`: a Node.js id is refused while
/// parsing, before authentication or any network call.
#[derive(Debug, Clone, clap::Args)]
pub(super) struct PublishAppIdArgs {
    /// WordPress (MHWP) application ID, from `hosting app list --app-type MHWP`.
    #[arg(long = "app-id", value_name = "APP_ID", value_parser = publishable_app_id)]
    pub(super) app_id: String,
}

fn publishable_app_id(value: &str) -> Result<String, String> {
    // A Node.js id gets its own message because the caller has somewhere to
    // go with it.
    if strip_id_prefix(value, NODEJS_ID_PREFIX).is_some() {
        return Err(
            "Node.js apps have no publish state. A Node.js app goes live with \
             `gddy hosting deployment publish --app-id <app-id>`."
                .to_owned(),
        );
    }
    if !is_wordpress_site_id(value) {
        return Err(
            "not a WordPress site id; expected MHWP- followed by 10 lower-case \
             letters or digits"
                .to_owned(),
        );
    }
    Ok(value.to_owned())
}

/// The edge route's id shape, `(?i)^mhwp-[a-z0-9]{10}$`: the prefix matches
/// case-insensitively, the ten characters after it do not. Anything else never
/// reaches the service, so it is refused here rather than sent out to collect
/// a 404.
fn is_wordpress_site_id(value: &str) -> bool {
    strip_id_prefix(value, MHWP_ID_PREFIX).is_some_and(|body| {
        body.len() == 10 && body.bytes().all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9'))
    })
}

pub(super) async fn set_publish_state(
    ctx: &cli_engine::CommandContext,
    app_id: String,
    published: bool,
) -> cli_engine::Result<CommandResult> {
    let client = make_client(ctx, &[DEPLOY_EXECUTE]).await?;
    let data = client
        .set_publish_state(&app_id, published)
        .await
        .map_err(|e| publish_state_err(e, published))?;
    let operation_id = data
        .get("operationId")
        .and_then(|v| v.as_str())
        .filter(|id| !id.is_empty())
        .map_or_else(NextActionParam::required, required_value);
    let description = if published {
        "Poll until the site is live"
    } else {
        "Poll until the site is behind its placeholder page"
    };
    let poll = next_action(
        "hosting operation get --operation-id <operation-id>",
        description,
    )
    .with_param("operation-id", operation_id);
    Ok(CommandResult::new(data).with_next_actions(vec![poll]))
}

const fn nodejs_fix(published: bool) -> &'static str {
    if published {
        NODEJS_PUBLISH_FIX
    } else {
        NODEJS_UNPUBLISH_FIX
    }
}

pub(super) fn publish_state_err(e: ClientError, published: bool) -> cli_engine::CliCoreError {
    let fix = error_issues(&e)
        .iter()
        .find_map(|issue| match issue.as_str() {
            "PUBLISH_LIMIT_EXCEEDED" if published => Some(
                "Your plan has no free published-site slot. Upgrade the plan in Airo App Builder, \
             or run `gddy hosting app unpublish --app-id <other-app-id>` to free one."
                    .to_owned(),
            ),
            "SUBSCRIPTION_INACTIVE" if published => Some(
                "Your Airo App Builder subscription is not active. Reactivate it, then run this \
             command again."
                    .to_owned(),
            ),
            "PUBLISH_STATE_CHANGE_IN_PROGRESS" => Some(
                "A publish-state change for this site or subscription is still being processed. \
             Poll any running operation with \
             `gddy hosting operation get --operation-id <operation-id>`, then retry."
                    .to_owned(),
            ),
            "NOT_APPLICABLE" => Some(nodejs_fix(published).to_owned()),
            _ => None,
        });
    match fix {
        Some(fix) => client_err_with_fix(e, fix),
        None => client_err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hosting::test_support::{cli, run_hosting};

    fn fix_for(status: u16, issue: &str, published: bool) -> Option<String> {
        let err = publish_state_err(
            ClientError::Http {
                status,
                body: format!(r#"{{"message":"m","details":[{{"issue":"{issue}"}}]}}"#),
            },
            published,
        );
        cli_engine::build_error_envelope(&err, "hosting").fix
    }

    #[test]
    fn cap_breach_points_at_the_plan_or_another_site() {
        let fix = fix_for(422, "PUBLISH_LIMIT_EXCEEDED", true);
        assert!(
            fix.as_deref()
                .is_some_and(|f| f.contains("hosting app unpublish")),
            "{fix:?}"
        );
        // An unpublish is never capped, so the hint does not apply to it.
        let fix = fix_for(422, "PUBLISH_LIMIT_EXCEEDED", false);
        assert!(
            fix.as_deref().is_none_or(|f| !f.contains("unpublish")),
            "{fix:?}"
        );
    }

    #[test]
    fn inactive_subscription_asks_to_reactivate_on_publish_only() {
        let fix = fix_for(403, "SUBSCRIPTION_INACTIVE", true);
        assert!(
            fix.as_deref()
                .is_some_and(|f| f.contains("subscription is not active")),
            "{fix:?}"
        );
        // Unpublish is allowed on an inactive subscription; a 403 there is a scope problem.
        let fix = fix_for(403, "SUBSCRIPTION_INACTIVE", false);
        assert!(
            fix.as_deref()
                .is_some_and(|f| !f.contains("subscription is not active")),
            "{fix:?}"
        );
    }

    #[test]
    fn change_in_progress_points_at_the_running_operation() {
        for published in [true, false] {
            let fix = fix_for(409, "PUBLISH_STATE_CHANGE_IN_PROGRESS", published);
            assert!(
                fix.as_deref().is_some_and(|f| {
                    f.contains("still being processed")
                        && f.contains("site or subscription")
                        && f.contains("hosting operation get --operation-id <operation-id>")
                }),
                "{fix:?}"
            );
        }
    }

    #[test]
    fn not_applicable_points_nodejs_at_deployment_publish() {
        let fix = fix_for(501, "NOT_APPLICABLE", true);
        assert!(
            fix.as_deref()
                .is_some_and(|f| f.contains("hosting deployment publish")),
            "{fix:?}"
        );
        // Node.js has no unpublish, so the hint must not send the caller to one.
        let fix = fix_for(501, "NOT_APPLICABLE", false);
        assert!(
            fix.as_deref()
                .is_some_and(|f| f.contains("WordPress") && !f.contains("deployment publish")),
            "{fix:?}"
        );
    }

    #[test]
    fn other_errors_keep_the_default_fix() {
        let fix = fix_for(404, "APP_NOT_FOUND", true);
        assert!(
            fix.as_deref().is_some_and(|f| !f.contains("unpublish")),
            "{fix:?}"
        );
    }

    #[test]
    fn publishable_app_id_rejects_nodejs_ids_case_insensitively() {
        for id in ["NODEJS-abc", "nodejs-abc", "NodeJS-abc"] {
            let err = publishable_app_id(id).expect_err(id);
            assert!(err.contains("hosting deployment publish"), "{id}: {err}");
        }
        // A Node.js id keeps its own message rather than the shape complaint.
        for id in ["NODEJS-abcdefghij", "nodejs-a1b2c3d4e5"] {
            let err = publishable_app_id(id).expect_err(id);
            assert!(!err.contains("not a WordPress site id"), "{id}: {err}");
        }
    }

    #[test]
    fn publishable_app_id_accepts_only_the_edge_id_shape() {
        // The prefix is case-insensitive at the edge, the body is not.
        for id in ["MHWP-a1b2c3d4e5", "mhwp-a1b2c3d4e5", "Mhwp-0123456789"] {
            assert_eq!(publishable_app_id(id).as_deref(), Ok(id));
        }
        for id in [
            "",
            "../x",
            "NODEJS",
            "MHWP",
            "MHWP-",
            "MHWP-TOOSHORT",
            "MHWP-A1B2C3D4E5",
            "MHWP-a1b2c3d4e",
            "MHWP-a1b2c3d4e56",
            "MHWP-a1b2c3d4e_",
            "x-MHWP-a1b2c3d4e5",
            "MHWP-a1b2c3d4e5/x",
            "MHWP\u{e9}",
            "NODEJS\u{e9}",
            "MHWP-a1b2c3d4\u{e9}",
            "MHWP-\u{ff41}1b2c3d4e5",
        ] {
            let err = publishable_app_id(id).expect_err(id);
            assert!(err.contains("not a WordPress site id"), "{id}: {err}");
        }
    }

    #[tokio::test]
    async fn nodejs_ids_are_refused_before_authentication() {
        let cli = cli(true);
        for command in ["publish", "unpublish"] {
            let out = run_hosting(&cli, &["app", command, "--app-id", "nodejs-abc"]).await;
            assert!(
                out.contains("no publish state") && out.contains("hosting deployment publish"),
                "{command}: {out}"
            );
            // Refused while parsing: auth (which has no provider in tests) was never reached.
            assert!(!out.contains("no provider registered"), "{command}: {out}");
        }
        let help = run_hosting(&cli, &["app", "publish", "--help"]).await;
        assert!(help.contains("--app-type MHWP"), "{help}");
    }
}
