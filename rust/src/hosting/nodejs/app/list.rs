use cli_engine::{CommandResult, CommandSpec, NextActionParam, RuntimeCommandSpec, Tier};

use crate::hosting::nodejs::{HostingAppSummary, client_err, make_client};
use crate::next_action::next_action;
use crate::scopes::HOSTING_APPS_READ as APPS_READ;

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_with_context(
        CommandSpec::new("list", "List Node.js hosting applications")
            .with_long("List all Node.js hosting applications in your account.")
            .with_system("hosting")
            .with_tier(Tier::Read)
            .with_scopes(&[APPS_READ])
            .with_default_fields("id,name,status")
            .with_output_schema::<HostingAppSummary>(),
        |ctx| async move {
            let client = make_client(&ctx, &[APPS_READ]).await?;
            let data = client.list_apps().await.map_err(client_err)?;
            Ok(CommandResult::new(data).with_next_actions(vec![
                next_action(
                    "hosting nodejs app get --app-id <app-id>",
                    "Get details for an application",
                )
                .with_param("app-id", NextActionParam::required()),
            ]))
        },
    )
}
