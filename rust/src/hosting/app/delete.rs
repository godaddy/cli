use cli_engine::{CommandResult, CommandSpec, RuntimeCommandSpec, Tier};
use serde_json::json;

use crate::hosting::common::{AppIdArgs, client_err, make_client};
use crate::next_action::next_action;
use crate::scopes::HOSTING_APPLICATION_DELETE as APP_DELETE;

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<AppIdArgs, _, _, _>(
        CommandSpec::from_args::<AppIdArgs>("delete", "Delete a hosting application")
            .with_long("Permanently delete a hosting application and all its associated data.")
            .with_system("hosting")
            .with_tier(Tier::Destructive)
            .mutates(true)
            .with_scopes(&[APP_DELETE]),
        |ctx, args: AppIdArgs| async move {
            let app_id = args.app_id;
            let client = make_client(&ctx, &[APP_DELETE]).await?;
            client.delete_app(&app_id).await.map_err(client_err)?;
            Ok(
                CommandResult::new(json!({ "deleted": true, "appId": app_id })).with_next_actions(
                    vec![next_action(
                        "hosting app list --app-type <type>",
                        "List remaining applications",
                    )],
                ),
            )
        },
    )
}
