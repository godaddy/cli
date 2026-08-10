use cli_engine::{CommandResult, CommandSpec, RuntimeCommandSpec, Tier};
use serde_json::json;

use crate::hosting::nodejs::{AppIdArgs, client_err, make_client};
use crate::next_action::next_action;
use crate::scopes::HOSTING_APPS_DELETE as APPS_DELETE;

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<AppIdArgs, _, _, _>(
        CommandSpec::from_args::<AppIdArgs>("delete", "Delete a Node.js hosting application")
            .with_long("Permanently delete a Node.js hosting application.")
            .with_system("hosting")
            .with_tier(Tier::Destructive)
            .mutates(true)
            .with_scopes(&[APPS_DELETE]),
        |ctx, args: AppIdArgs| async move {
            let app_id = args.app_id;
            let client = make_client(&ctx, &[APPS_DELETE]).await?;
            client.delete_app(&app_id).await.map_err(client_err)?;
            Ok(
                CommandResult::new(json!({ "deleted": true, "appId": app_id })).with_next_actions(
                    vec![next_action(
                        "hosting nodejs app list",
                        "List remaining applications",
                    )],
                ),
            )
        },
    )
}
