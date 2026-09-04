use cli_engine::{CommandResult, CommandSpec, NextActionParam, RuntimeCommandSpec, Tier};

use crate::hosting::common::{client_err, make_client};
use crate::next_action::next_action;
use crate::scopes::HOSTING_APPLICATION_READ as APP_READ;

#[derive(Debug, Clone, clap::Args)]
struct DeploymentGetArgs {
    /// Application ID.
    #[arg(long = "app-id", value_name = "APP_ID")]
    app_id: String,

    /// Deployment ID.
    #[arg(long = "deployment-id", value_name = "DEPLOYMENT_ID")]
    deployment_id: String,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<DeploymentGetArgs, _, _, _>(
        CommandSpec::from_args::<DeploymentGetArgs>("get", "Get a deployment")
            .with_long("Get the current status and details of a specific deployment.")
            .with_system("hosting")
            .with_tier(Tier::Read)
            .with_scopes(&[APP_READ]),
        |ctx, args: DeploymentGetArgs| async move {
            let app_id = args.app_id.clone();
            let deployment_id = args.deployment_id;
            let client = make_client(&ctx, &[APP_READ]).await?;
            let data = client
                .get_deployment(&app_id, &deployment_id)
                .await
                .map_err(client_err)?;
            Ok(CommandResult::new(data).with_next_actions(vec![
                next_action(
                    "hosting operation get --operation-id <id>",
                    "Poll the operation for this deployment",
                )
                .with_param("operation-id", NextActionParam::required()),
            ]))
        },
    )
}
