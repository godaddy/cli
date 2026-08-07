//! `gddy hosting nodejs deployment` — promote preview to live and view deployment history.

use cli_engine::{
    CommandResult, CommandSpec, GroupSpec, NextActionParam, RuntimeCommandSpec, RuntimeGroupSpec,
    Tier,
};

use crate::hosting::nodejs::{AppIdArgs, client_err, make_client};
use crate::next_action::next_action;
use crate::scopes::{HOSTING_APPS_READ as APPS_READ, HOSTING_DEPLOY_EXECUTE as DEPLOY_EXECUTE};

pub(super) fn group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(GroupSpec::new(
        "deployment",
        "Promote preview to live and view deployment history",
    ))
    .with_command(list_command())
    .with_command(publish_command())
}

#[derive(Debug, Clone, clap::Args)]
struct DeploymentListArgs {
    /// Application ID.
    #[arg(long = "app-id", value_name = "APP_ID")]
    app_id: String,

    // Local override of the engine's global `--limit`, typed `i64` to match
    // it (see `domain::suggest::SuggestArgs::limit` for the full rationale —
    // a differently-typed override panics with a downcast mismatch reading
    // the root `ArgMatches`).
    #[arg(
        long,
        value_name = "N",
        help = "Maximum deployments to return (1-50)",
        allow_negative_numbers = true,
        value_parser = clap::value_parser!(i64).range(1..=50)
    )]
    limit: Option<i64>,
}

fn list_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<DeploymentListArgs, _, _, _>(
        CommandSpec::from_args::<DeploymentListArgs>("list", "List application deployments")
            .with_long("List deployments for a Node.js hosting application.")
            .with_system("hosting")
            .with_tier(Tier::Read)
            .with_scopes(&[APPS_READ]),
        |ctx, args: DeploymentListArgs| async move {
            let app_id = args.app_id;
            let limit = args.limit.and_then(|n| u32::try_from(n).ok());
            let client = make_client(&ctx, &[APPS_READ]).await?;
            let data = client
                .list_deployments(&app_id, limit)
                .await
                .map_err(client_err)?;
            Ok(CommandResult::new(data).with_next_actions(vec![
                next_action(
                    "hosting nodejs deployment publish --app-id <app-id>",
                    "Publish the latest source",
                )
                .with_param("app-id", NextActionParam::value(app_id)),
            ]))
        },
    )
}

fn publish_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<AppIdArgs, _, _, _>(
        CommandSpec::from_args::<AppIdArgs>(
            "publish",
            "Promote the app's preview environment to live",
        )
        .with_long(
            "Promote the app's current preview environment to live, making it \
            available to users. Run after a source upload or git import job reaches \
            `complete`. Poll `status` (variants[].lifecycleState on the publish \
            variant) and `deployment list` for progress.",
        )
        .with_system("hosting")
        .with_tier(Tier::Mutate)
        .mutates(true)
        .with_scopes(&[DEPLOY_EXECUTE]),
        |ctx, args: AppIdArgs| async move {
            let app_id = args.app_id;
            let client = make_client(&ctx, &[DEPLOY_EXECUTE]).await?;
            let data = client.publish_app(&app_id).await.map_err(client_err)?;
            Ok(CommandResult::new(data).with_next_actions(vec![
                next_action(
                    "hosting nodejs status --app-id <app-id>",
                    "Check application status",
                )
                .with_param("app-id", NextActionParam::value(app_id.clone())),
                next_action(
                    "hosting nodejs deployment list --app-id <app-id>",
                    "List deployments",
                )
                .with_param("app-id", NextActionParam::value(app_id)),
            ]))
        },
    )
}

#[cfg(test)]
mod tests {
    use super::list_command;

    /// Builds a clap command from the real `--limit` arg rather than a hand-rolled copy.
    fn deployment_list_clap_command() -> clap::Command {
        clap::Command::new("list").args(list_command().spec.args)
    }

    #[test]
    fn limit_rejects_out_of_range_and_negative_values() {
        for bad in ["0", "51", "-1"] {
            let err = deployment_list_clap_command()
                .try_get_matches_from(["list", "--app-id", "app-1", "--limit", bad])
                .expect_err("out-of-range --limit should be rejected");
            assert_eq!(
                err.kind(),
                clap::error::ErrorKind::ValueValidation,
                "--limit {bad} should fail range validation, got: {err}"
            );
        }
    }

    #[test]
    fn limit_accepts_the_full_valid_range() {
        for good in ["1", "50"] {
            deployment_list_clap_command()
                .try_get_matches_from(["list", "--app-id", "app-1", "--limit", good])
                .expect("value within the valid --limit range should be accepted");
        }
    }
}
