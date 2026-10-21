use cli_engine::{
    CommandResult, CommandSpec, CursorConfig, NextActionParam, RuntimeCommandSpec, Tier,
};
use serde_json::json;

use crate::hosting::common::{
    HostingDeploymentSummary, client_err, make_client, page_continuation, page_items, page_limit,
};
use crate::next_action::next_action;
use crate::scopes::HOSTING_APPLICATION_READ as APP_READ;

#[derive(Debug, Clone, clap::Args)]
struct DeploymentListArgs {
    /// Application ID.
    #[arg(long = "app-id", value_name = "APP_ID")]
    app_id: String,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<DeploymentListArgs, _, _, _>(
        CommandSpec::from_args::<DeploymentListArgs>("list", "List deployments for an application")
            .with_long(
                "List deployments for a hosting application, newest first, one page at a time. \
                 Use --limit to set the page size and --continue with the token from a \
                 page's cursor to fetch the next.",
            )
            .with_system("hosting")
            .with_tier(Tier::Read)
            .with_scopes(&[APP_READ])
            .with_default_fields("deploymentId,status,createdAt")
            .with_output_schema::<HostingDeploymentSummary>()
            .with_cursor(CursorConfig::new(50, 100)),
        |ctx, args: DeploymentListArgs| async move {
            let app_id = args.app_id;
            let client = make_client(&ctx, &[APP_READ]).await?;

            let response = client
                .list_deployments(
                    &app_id,
                    ctx.middleware.continue_token.as_deref(),
                    page_limit(ctx.middleware.cursor_limit),
                )
                .await
                .map_err(client_err)?;

            Ok(CommandResult::new(json!(page_items(&response)))
                .with_cursor(page_continuation(&response))
                .with_next_actions(vec![
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

#[cfg(test)]
mod tests {
    use cli_engine::CursorConfig;

    use super::command;

    #[test]
    fn opts_into_cursor_pagination_with_a_default_and_a_max_limit() {
        assert_eq!(command().spec.cursor, Some(CursorConfig::new(50, 100)));
    }
}
