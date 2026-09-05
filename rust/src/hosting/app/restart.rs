use cli_engine::{CommandResult, CommandSpec, NextActionParam, RuntimeCommandSpec, Tier};

use crate::hosting::common::{client_err, make_client};
use crate::next_action::next_action;
use crate::scopes::HOSTING_DEPLOYMENT_EXECUTE as DEPLOY_EXECUTE;

#[derive(Debug, Clone, clap::Args)]
struct AppRestartArgs {
    /// Application ID.
    #[arg(long = "app-id", value_name = "APP_ID")]
    app_id: String,

    /// Environment to restart (PREVIEW or PUBLISH).
    #[arg(long, value_name = "VARIANT", value_parser = ["PREVIEW", "PUBLISH"])]
    variant: String,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<AppRestartArgs, _, _, _>(
        CommandSpec::from_args::<AppRestartArgs>("restart", "Restart an application environment")
            .with_long(
                "Restart the PREVIEW or PUBLISH environment of a hosting application. \
                 Check `hosting app status` after restarting to confirm the environment \
                 returns to ACTIVE.",
            )
            .with_system("hosting")
            .with_tier(Tier::Mutate)
            .mutates(true)
            .with_scopes(&[DEPLOY_EXECUTE]),
        |ctx, args: AppRestartArgs| async move {
            let app_id = args.app_id;
            let variant = args.variant;
            let client = make_client(&ctx, &[DEPLOY_EXECUTE]).await?;
            let data = client
                .restart_app(&app_id, &variant)
                .await
                .map_err(client_err)?;
            Ok(CommandResult::new(data).with_next_actions(vec![
                next_action(
                    "hosting app status --app-id <app-id>",
                    "Check environment status after restart",
                )
                .with_param("app-id", NextActionParam::value(app_id)),
            ]))
        },
    )
}
