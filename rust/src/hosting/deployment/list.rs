use cli_engine::{CommandResult, CommandSpec, NextActionParam, RuntimeCommandSpec, Tier};
use serde_json::{Value, json};

use super::common::plan_failure;
use crate::hosting::common::{HostingDeploymentSummary, client_err, make_client, next_page_token};
use crate::next_action::next_action;
use crate::scopes::HOSTING_APPLICATION_READ as APP_READ;

#[derive(Debug, Clone, clap::Args)]
struct DeploymentListArgs {
    /// Application ID.
    #[arg(long = "app-id", value_name = "APP_ID")]
    app_id: String,

    /// Maximum number of deployments to return. Omit to return all.
    #[arg(long, value_name = "N", value_parser = clap::value_parser!(u32).range(1..))]
    limit: Option<u32>,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<DeploymentListArgs, _, _, _>(
        CommandSpec::from_args::<DeploymentListArgs>("list", "List deployments for an application")
            .with_long(
                "List all deployments for a hosting application, newest first. \
                 Results are autopaginated. Use --limit to cap the total returned. \
                 To see why a deployment failed, run `hosting deployment get` for it.",
            )
            .with_system("hosting")
            .with_tier(Tier::Read)
            .with_scopes(&[APP_READ])
            .with_default_fields("deploymentId,status,createdAt")
            .with_output_schema::<HostingDeploymentSummary>(),
        |ctx, args: DeploymentListArgs| async move {
            let app_id = args.app_id;
            let limit = args.limit;
            let client = make_client(&ctx, &[APP_READ]).await?;

            let mut all_items: Vec<Value> = Vec::new();
            let mut page_token: Option<String> = None;

            loop {
                let page_limit =
                    limit.map(|cap| cap.saturating_sub(all_items.len() as u32).min(100));

                let response = client
                    .list_deployments(&app_id, page_token.as_deref(), page_limit)
                    .await
                    .map_err(client_err)?;

                if let Some(items) = response.get("items").and_then(|v| v.as_array()) {
                    all_items.extend(items.iter().cloned());
                }

                if limit.is_some_and(|cap| all_items.len() >= cap as usize) {
                    all_items.truncate(limit.expect("checked") as usize);
                    break;
                }

                match next_page_token(&response) {
                    Some(token) => page_token = Some(token),
                    None => break,
                }
            }

            Ok(result(all_items, &app_id))
        },
    )
}

/// The deployments, and the way to a deployment's details. When the newest
/// one failed because of the app's Airo App Builder plan, the plan is what
/// stands between the app and going live, so that is said too.
fn result(items: Vec<Value>, app_id: &str) -> CommandResult {
    let details = next_action(
        "hosting deployment get --app-id <app-id> --deployment-id <id>",
        "Get full details of a deployment",
    )
    .with_param("app-id", NextActionParam::required())
    .with_param("deployment-id", NextActionParam::required());
    let latest = items.first().and_then(|newest| {
        Some((
            newest["deploymentId"].as_str()?,
            plan_failure(newest, app_id)?,
        ))
    });
    let Some((deployment_id, (warning, publish))) = latest else {
        return CommandResult::new(json!(items)).with_next_actions(vec![details]);
    };
    let warning = format!("The latest deployment, {deployment_id}, failed. {warning}");
    CommandResult::new(json!(items))
        .with_warning(warning)
        .with_next_actions(vec![publish, details])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deployment(id: &str, status: &str, issue: Option<&str>) -> Value {
        let mut deployment = json!({
            "deploymentId": id,
            "status": status,
            "createdAt": "2026-11-24T10:00:00Z",
            "updatedAt": "2026-11-24T10:01:00Z",
            "links": []
        });
        if let Some(issue) = issue {
            deployment["error"] = json!({
                "name": "DeploymentFailed",
                "message": "m",
                "details": [{ "issue": issue }],
                "links": [{ "rel": "upgrade", "href": "https://www.example.com/manage-plan" }]
            });
        }
        deployment
    }

    #[test]
    fn a_latest_deployment_refused_by_the_plan_is_called_out() {
        let result = result(
            vec![
                deployment("dep-2", "FAILED", Some("PLAN_UPGRADE_REQUIRED")),
                deployment("dep-1", "COMPLETED", None),
            ],
            "NODEJS-aBcDeFgHiJ",
        );
        assert_eq!(
            result.metadata.warnings,
            [
                "The latest deployment, dep-2, failed. Your Airo App Builder plan does not let \
                 this app go live. Upgrade the plan, then publish the app again. Upgrade page: \
                 https://www.example.com/manage-plan"
            ]
        );
        let commands: Vec<&str> = result
            .metadata
            .next_actions
            .iter()
            .map(|a| a.command.as_str())
            .collect();
        assert_eq!(
            commands,
            [
                "gddy hosting deployment publish --app-id <app-id>",
                "gddy hosting deployment get --app-id <app-id> --deployment-id <id>"
            ]
        );
        assert_eq!(
            result.metadata.next_actions[0].params["app-id"]
                .value
                .as_deref(),
            Some("NODEJS-aBcDeFgHiJ")
        );
    }

    #[test]
    fn an_older_or_other_failure_adds_nothing() {
        for items in [
            vec![
                deployment("dep-2", "COMPLETED", None),
                deployment("dep-1", "FAILED", Some("PLAN_UPGRADE_REQUIRED")),
            ],
            vec![deployment("dep-1", "FAILED", Some("DEPLOY_FAILED"))],
            vec![],
        ] {
            let result = result(items.clone(), "NODEJS-aBcDeFgHiJ");
            assert!(result.metadata.warnings.is_empty(), "{items:?}");
            assert_eq!(result.metadata.next_actions.len(), 1, "{items:?}");
            assert_eq!(result.data, json!(items));
        }
    }
}
