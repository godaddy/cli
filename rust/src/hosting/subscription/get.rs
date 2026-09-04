use cli_engine::{CommandResult, CommandSpec, NextActionParam, RuntimeCommandSpec, Tier};

use crate::hosting::common::{AppIdArgs, client_err, make_client};
use crate::next_action::next_action;
use crate::scopes::HOSTING_SUBSCRIPTION_READ as SUB_READ;

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<AppIdArgs, _, _, _>(
        CommandSpec::from_args::<AppIdArgs>(
            "get",
            "Get the subscription attached to an application",
        )
        .with_long(
            "Get the hosting plan subscription currently attached to a specific application.",
        )
        .with_system("hosting")
        .with_tier(Tier::Read)
        .with_scopes(&[SUB_READ]),
        |ctx, args: AppIdArgs| async move {
            let app_id = args.app_id;
            let client = make_client(&ctx, &[SUB_READ]).await?;
            let data = client
                .get_app_subscription(&app_id)
                .await
                .map_err(client_err)?;
            Ok(CommandResult::new(data).with_next_actions(vec![
                next_action(
                    "hosting subscription attach --app-id <app-id> --subscription-id <id>",
                    "Attach a different subscription",
                )
                .with_param("app-id", NextActionParam::value(app_id))
                .with_param("subscription-id", NextActionParam::required()),
            ]))
        },
    )
}
