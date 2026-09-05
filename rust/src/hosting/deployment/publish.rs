use cli_engine::{CommandResult, CommandSpec, NextActionParam, RuntimeCommandSpec, Tier};

use crate::hosting::common::{AppIdArgs, HostingDeploymentSummary, client_err, make_client};
use crate::next_action::next_action;
use crate::scopes::HOSTING_DEPLOYMENT_EXECUTE as DEPLOY_EXECUTE;

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<AppIdArgs, _, _, _>(
        CommandSpec::from_args::<AppIdArgs>("publish", "Trigger a deployment")
            .with_long(
                "Promote the current PREVIEW build to PUBLISH. Hosting uses a two-stage model: \
                 `hosting source upload` (or `hosting source github`) refreshes PREVIEW, \
                 and `hosting deployment publish` builds that source and rolls it out to PUBLISH — \
                 which is why there is no --variant flag here. Returns immediately with a \
                 deployment ID; poll `hosting deployment get` until status is COMPLETED or FAILED.",
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
                .map_err(client_err)?;
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
