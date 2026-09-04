use cli_engine::{CommandResult, CommandSpec, NextActionParam, RuntimeCommandSpec, Tier};

use crate::hosting::common::{AppIdArgs, HostingApplication, client_err, make_client};
use crate::next_action::next_action;
use crate::scopes::HOSTING_APPLICATION_READ as APP_READ;

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<AppIdArgs, _, _, _>(
        CommandSpec::from_args::<AppIdArgs>("get", "Get a hosting application")
            .with_long("Get details for a single hosting application by ID.")
            .with_system("hosting")
            .with_tier(Tier::Read)
            .with_scopes(&[APP_READ])
            .with_output_schema::<HostingApplication>(),
        |ctx, args: AppIdArgs| async move {
            let app_id = args.app_id;
            let client = make_client(&ctx, &[APP_READ]).await?;
            let data = client.get_app(&app_id).await.map_err(client_err)?;
            Ok(CommandResult::new(data).with_next_actions(vec![
                next_action(
                    "hosting app status --app-id <app-id>",
                    "Get runtime status for this application",
                )
                .with_param("app-id", NextActionParam::value(app_id.clone())),
                next_action(
                    "hosting deployment list --app-id <app-id>",
                    "List deployments for this application",
                )
                .with_param("app-id", NextActionParam::value(app_id)),
            ]))
        },
    )
}
