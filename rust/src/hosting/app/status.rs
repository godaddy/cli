use cli_engine::{CommandResult, CommandSpec, NextActionParam, RuntimeCommandSpec, Tier};

use crate::hosting::common::{AppIdArgs, client_err, make_client};
use crate::next_action::next_action;
use crate::scopes::HOSTING_APPLICATION_READ as APP_READ;

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<AppIdArgs, _, _, _>(
        CommandSpec::from_args::<AppIdArgs>("status", "Get application runtime status")
            .with_long(
                "Get the runtime status of a hosting application's environments \
                 (preview and publish). Use this to check whether an environment \
                 is ACTIVE, IDLE, or in a transitional state after a restart or deployment.",
            )
            .with_system("hosting")
            .with_tier(Tier::Read)
            .with_scopes(&[APP_READ]),
        |ctx, args: AppIdArgs| async move {
            let app_id = args.app_id;
            let client = make_client(&ctx, &[APP_READ]).await?;
            let data = client.get_app_status(&app_id).await.map_err(client_err)?;
            Ok(CommandResult::new(data).with_next_actions(vec![
                next_action(
                    "hosting deployment list --app-id <app-id>",
                    "List deployments for this application",
                )
                .with_param("app-id", NextActionParam::value(app_id.clone())),
                next_action(
                    "hosting app restart --app-id <app-id> --variant <variant>",
                    "Restart an environment",
                )
                .with_param("app-id", NextActionParam::value(app_id)),
            ]))
        },
    )
}
