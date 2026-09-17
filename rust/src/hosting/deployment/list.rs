use cli_engine::{CommandResult, CommandSpec, NextActionParam, RuntimeCommandSpec, Tier};
use serde_json::{Value, json};

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
                 Results are autopaginated. Use --limit to cap the total returned.",
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

            Ok(CommandResult::new(json!(all_items)).with_next_actions(vec![
                next_action(
                    "hosting deployment get --app-id <app-id> --deployment-id <id>",
                    "Get full details of a deployment",
                )
                .with_param("app-id", NextActionParam::required())
                .with_param("deployment-id", NextActionParam::required()),
            ]))
        },
    )
}
