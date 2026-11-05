use cli_engine::{
    CommandResult, CommandSpec, NextAction, NextActionParam, RuntimeCommandSpec, Tier,
};
use serde_json::Value;

use crate::hosting::common::{HostingDeploymentSummary, client_err, make_client};
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
            .with_long("Get the current status and details of a specific deployment.")
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
                .map_err(client_err)?;
            let next_actions = next_actions(&data, &app_id);
            Ok(CommandResult::new(data).with_next_actions(next_actions))
        },
    )
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
