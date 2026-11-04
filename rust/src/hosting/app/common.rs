//! Shared body of `hosting app publish` and `hosting app unpublish`.

use cli_engine::{CommandResult, NextActionParam};

use crate::hosting::client::{ClientError, HostingClient};
use crate::hosting::common::{
    HostingAppType, client_err, client_err_with_fix, error_issues, strip_id_prefix,
};
use crate::hosting::deployment::publish::deploy;
use crate::next_action::{next_action, required_value};

const NOT_A_NODEJS_APP: &str =
    "not a Node.js app id; expected NODEJS- followed by letters, digits, `_` or `-`";
const NOT_A_WORDPRESS_SITE: &str =
    "not a WordPress site id; expected MHWP- followed by 10 lower-case letters or digits";

/// `--app-id` for `publish`: the product is read off the id's prefix while
/// parsing, so a malformed id is refused before authentication or any
/// network call.
#[derive(Debug, Clone, clap::Args)]
pub(super) struct PublishArgs {
    /// Application ID, from `hosting app list --app-type <type>`: a Node.js app
    /// (NODEJS-…) or a WordPress site (MHWP-…).
    #[arg(long = "app-id", value_name = "APP_ID", value_parser = publishable_app)]
    pub(super) app: PublishableApp,
}

/// `--app-id` for `unpublish`: only a WordPress site has a placeholder page to
/// go back to, so any other id is refused while parsing.
#[derive(Debug, Clone, clap::Args)]
pub(super) struct UnpublishArgs {
    /// WordPress (MHWP) application ID, from `hosting app list --app-type MHWP`.
    #[arg(long = "app-id", value_name = "APP_ID", value_parser = wordpress_site_id)]
    pub(super) app_id: String,
}

/// An app id together with the product its prefix names.
#[derive(Debug, Clone)]
pub(super) struct PublishableApp {
    app_type: HostingAppType,
    id: String,
}

fn publishable_app(value: &str) -> Result<PublishableApp, String> {
    let app_type = HostingAppType::of_id(value).ok_or(
        "not an app id; expected a Node.js app (NODEJS-…) or a WordPress site (MHWP-…) id, \
         from `gddy hosting app list --app-type <type>`",
    )?;
    let (well_formed, expected) = match app_type {
        HostingAppType::Nodejs => (is_nodejs_app_id(value), NOT_A_NODEJS_APP),
        HostingAppType::Mhwp => (is_wordpress_site_id(value), NOT_A_WORDPRESS_SITE),
    };
    if !well_formed {
        return Err(expected.to_owned());
    }
    Ok(PublishableApp {
        app_type,
        id: value.to_owned(),
    })
}

fn wordpress_site_id(value: &str) -> Result<String, String> {
    if HostingAppType::of_id(value) == Some(HostingAppType::Nodejs) {
        return Err(
            "Node.js apps cannot be unpublished; only a WordPress site can go back to a \
             placeholder page."
                .to_owned(),
        );
    }
    if !is_wordpress_site_id(value) {
        return Err(NOT_A_WORDPRESS_SITE.to_owned());
    }
    Ok(value.to_owned())
}

/// A Node.js id ends in a nanoid. Only its alphabet is checked, which keeps the
/// id URL-safe; the contract's examples disagree on the length, so the API
/// judges the rest.
fn is_nodejs_app_id(value: &str) -> bool {
    strip_id_prefix(value, HostingAppType::Nodejs.id_prefix()).is_some_and(|body| {
        !body.is_empty()
            && body
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
    })
}

/// The edge route's id shape, `(?i)^mhwp-[a-z0-9]{10}$`: the prefix matches
/// case-insensitively, the ten characters after it do not. Anything else never
/// reaches the service, so it is refused here rather than sent out to collect
/// a 404.
fn is_wordpress_site_id(value: &str) -> bool {
    strip_id_prefix(value, HostingAppType::Mhwp.id_prefix()).is_some_and(|body| {
        body.len() == 10 && body.bytes().all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9'))
    })
}

/// Takes `app` live: a Node.js app by building and deploying its latest
/// source, exactly as `hosting deployment publish` does, and a WordPress site
/// by switching it from its placeholder page.
pub(super) async fn publish(
    client: &HostingClient,
    app: PublishableApp,
) -> cli_engine::Result<CommandResult> {
    match app.app_type {
        HostingAppType::Nodejs => deploy(client, app.id).await,
        HostingAppType::Mhwp => set_publish_state(client, app.id, true).await,
    }
}

pub(super) async fn set_publish_state(
    client: &HostingClient,
    app_id: String,
    published: bool,
) -> cli_engine::Result<CommandResult> {
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

fn publish_state_err(e: ClientError, published: bool) -> cli_engine::CliCoreError {
    let fix = error_issues(&e)
        .iter()
        .find_map(|issue| match issue.as_str() {
            "PUBLISH_LIMIT_EXCEEDED" if published => Some(
                "Your plan has no free published-site slot. Upgrade the plan in Airo App Builder, \
             or run `gddy hosting app unpublish --app-id <other-app-id>` to free one.",
            ),
            "SUBSCRIPTION_INACTIVE" if published => Some(
                "Your Airo App Builder subscription is not active. Reactivate it, then run this \
             command again.",
            ),
            "PUBLISH_STATE_CHANGE_IN_PROGRESS" => Some(
                "A publish-state change for this site or subscription is still being processed. \
             Poll any running operation with \
             `gddy hosting operation get --operation-id <operation-id>`, then retry.",
            ),
            _ => None,
        });
    match fix {
        Some(fix) => client_err_with_fix(e, fix),
        None => client_err(e),
    }
}

#[cfg(test)]
mod tests {
    use httpmock::prelude::*;
    use serde_json::json;

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

    /// Only WordPress ids reach the publish-state call, so a `501` there gets
    /// the generic hint rather than one about Node.js.
    #[test]
    fn not_applicable_keeps_the_generic_fix() {
        for published in [true, false] {
            let fix = fix_for(501, "NOT_APPLICABLE", published);
            assert!(
                fix.as_deref()
                    .is_some_and(|f| f.contains("not supported for this app type")),
                "{fix:?}"
            );
        }
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
    fn publish_reads_the_product_off_the_id() {
        for (id, app_type) in [
            ("NODEJS-aBcDeFgHiJ", HostingAppType::Nodejs),
            ("nodejs-v1StcMHAbqMs", HostingAppType::Nodejs),
            ("NodeJS-a_b-c", HostingAppType::Nodejs),
            ("MHWP-a1b2c3d4e5", HostingAppType::Mhwp),
            ("mhwp-0123456789", HostingAppType::Mhwp),
        ] {
            let app = publishable_app(id).expect(id);
            assert_eq!(app.app_type, app_type, "{id}");
            assert_eq!(app.id, id);
        }
    }

    #[test]
    fn publish_refuses_malformed_ids_with_a_message_for_their_product() {
        for id in [
            "NODEJS-",
            "NODEJS-a/b",
            "NODEJS-a b",
            "NODEJS-a?b",
            "NODEJS-ab%2F",
            "NODEJS-\u{e9}t\u{e9}",
        ] {
            let err = publishable_app(id).expect_err(id);
            assert!(err.contains("not a Node.js app id"), "{id}: {err}");
        }
        for id in [
            "MHWP-",
            "MHWP-TOOSHORT",
            "MHWP-A1B2C3D4E5",
            "MHWP-a1b2c3d4e",
            "MHWP-a1b2c3d4e56",
            "MHWP-a1b2c3d4e_",
            "MHWP-a1b2c3d4e5/x",
            "MHWP-a1b2c3d4\u{e9}",
            "MHWP-\u{ff41}1b2c3d4e5",
        ] {
            let err = publishable_app(id).expect_err(id);
            assert!(err.contains("not a WordPress site id"), "{id}: {err}");
        }
        for id in [
            "",
            "../x",
            "NODEJS",
            "MHWP",
            "MHWP\u{e9}",
            "NODEJS\u{e9}",
            "x-MHWP-a1b2c3d4e5",
            "app-1",
        ] {
            let err = publishable_app(id).expect_err(id);
            assert!(err.contains("not an app id"), "{id}: {err}");
        }
    }

    #[test]
    fn unpublish_takes_wordpress_sites_only() {
        for id in ["MHWP-a1b2c3d4e5", "mhwp-a1b2c3d4e5", "Mhwp-0123456789"] {
            assert_eq!(wordpress_site_id(id).as_deref(), Ok(id));
        }
        for id in ["NODEJS-aBcDeFgHiJ", "nodejs-abc", "NodeJS-"] {
            let err = wordpress_site_id(id).expect_err(id);
            assert!(err.contains("cannot be unpublished"), "{id}: {err}");
        }
        for id in ["", "../x", "MHWP", "MHWP-TOOSHORT", "x-MHWP-a1b2c3d4e5"] {
            let err = wordpress_site_id(id).expect_err(id);
            assert!(err.contains("not a WordPress site id"), "{id}: {err}");
        }
    }

    #[tokio::test]
    async fn publish_deploys_a_nodejs_app_like_deployment_publish() {
        let server = MockServer::start_async().await;
        let deployment = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v1/hosting/apps/NODEJS-aBcDeFgHiJ/deployments");
                then.status(202)
                    .json_body(json!({ "deploymentId": "dep-1", "status": "PENDING" }));
            })
            .await;
        let publish_state = server
            .mock_async(|when, then| {
                when.method(PUT);
                then.status(500);
            })
            .await;

        let client = HostingClient::new(server.base_url(), "test-token");
        let app = publishable_app("NODEJS-aBcDeFgHiJ").expect("Node.js id");
        let result = publish(&client, app).await.expect("publish");

        deployment.assert_async().await;
        publish_state.assert_calls_async(0).await;
        assert_eq!(result.data["deploymentId"], "dep-1");
        let actions = &result.metadata.next_actions;
        assert_eq!(
            actions[0].command,
            "gddy hosting deployment get --app-id <app-id> --deployment-id <id>"
        );
    }

    #[tokio::test]
    async fn publish_switches_a_wordpress_site_live() {
        let server = MockServer::start_async().await;
        let publish_state = server
            .mock_async(|when, then| {
                when.method(PUT)
                    .path("/v1/hosting/apps/MHWP-a1b2c3d4e5/publish-state")
                    .json_body(json!({ "published": true }));
                then.status(202).json_body(json!({
                    "operationId": "MHWP-p9x8y7z6", "status": "PENDING",
                    "type": "SET_PUBLISH_STATE", "links": []
                }));
            })
            .await;
        let deployment = server
            .mock_async(|when, then| {
                when.method(POST);
                then.status(500);
            })
            .await;

        let client = HostingClient::new(server.base_url(), "test-token");
        let app = publishable_app("MHWP-a1b2c3d4e5").expect("WordPress id");
        let result = publish(&client, app).await.expect("publish");

        publish_state.assert_async().await;
        deployment.assert_calls_async(0).await;
        assert_eq!(result.data["operationId"], "MHWP-p9x8y7z6");
        let actions = &result.metadata.next_actions;
        assert_eq!(
            actions[0].command,
            "gddy hosting operation get --operation-id <operation-id>"
        );
    }

    /// A Node.js publish keeps the deployment command's error hints, never the
    /// WordPress plan advice.
    #[tokio::test]
    async fn a_nodejs_publish_keeps_the_deployment_error_hints() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v1/hosting/apps/NODEJS-aBcDeFgHiJ/deployments");
                then.status(422).json_body(json!({
                    "message": "The plan's live-site limit is reached.",
                    "details": [{ "issue": "PUBLISH_LIMIT_EXCEEDED" }]
                }));
            })
            .await;

        let client = HostingClient::new(server.base_url(), "test-token");
        let app = publishable_app("NODEJS-aBcDeFgHiJ").expect("Node.js id");
        let err = publish(&client, app).await.expect_err("422");
        let envelope = cli_engine::build_error_envelope(&err, "hosting");
        assert_eq!(
            envelope.fix.as_deref(),
            Some("Run: gddy hosting subscription list --hosting-product=WEB_HOSTING")
        );
    }

    #[tokio::test]
    async fn ids_are_checked_before_authentication() {
        let cli = cli(true);
        // Valid for publish: parses, then stops at auth (no provider in tests).
        for id in ["NODEJS-aBcDeFgHiJ", "MHWP-a1b2c3d4e5"] {
            let out = run_hosting(&cli, &["app", "publish", "--app-id", id]).await;
            assert!(out.contains("no provider registered"), "{id}: {out}");
        }
        // Refused while parsing: auth was never reached.
        let cases: [(&str, &str, &str); 3] = [
            ("publish", "app-1", "not an app id"),
            ("unpublish", "NODEJS-aBcDeFgHiJ", "cannot be unpublished"),
            ("unpublish", "MHWP-TOOSHORT", "not a WordPress site id"),
        ];
        for (command, id, message) in cases {
            let out = run_hosting(&cli, &["app", command, "--app-id", id]).await;
            assert!(out.contains(message), "{command} {id}: {out}");
            assert!(
                !out.contains("no provider registered"),
                "{command} {id}: {out}"
            );
        }
    }
}
