use cli_engine::{CommandResult, CursorConfig, NextActionParam, RuntimeCommandSpec, Tier};
use serde_json::json;

use crate::hosting::common::{
    HostingAppSummary, HostingAppType, app_type_command, client_err, make_client,
    page_continuation, page_items, page_limit, supported_app_types,
};
use crate::next_action::next_action;
use crate::scopes::HOSTING_APPLICATION_READ as APP_READ;

#[derive(Debug, Clone, clap::Args)]
struct AppListArgs {
    /// Hosting product.
    #[arg(long = "app-type", value_name = "TYPE", ignore_case = true)]
    app_type: HostingAppType,
}

pub(super) fn command(mhwp: bool) -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<AppListArgs, _, _, _>(
        app_type_command::<AppListArgs>("list", "List hosting applications", mhwp)
            .with_long(format!(
                "List hosting applications of a given type, one page at a time. Use --limit to \
                 set the page size and --continue with the token from a page's cursor to \
                 fetch the next.\n\
                 \n\
                 --app-type is required. Currently supported: {}.",
                supported_app_types(mhwp)
            ))
            .with_system("hosting")
            .with_tier(Tier::Read)
            .with_scopes(&[APP_READ])
            .with_default_fields("id,name,status")
            .with_output_schema::<HostingAppSummary>()
            .with_cursor(CursorConfig::new(50, 100)),
        |ctx, args: AppListArgs| async move {
            let app_type = args.app_type;
            let client = make_client(&ctx, &[APP_READ]).await?;

            let response = client
                .list_apps(
                    app_type.as_str(),
                    ctx.middleware.continue_token.as_deref(),
                    page_limit(ctx.middleware.cursor_limit),
                )
                .await
                .map_err(client_err)?;

            Ok(CommandResult::new(json!(page_items(&response)))
                .with_cursor(page_continuation(&response))
                .with_next_actions(vec![
                    next_action(
                        "hosting app get --app-id <app-id>",
                        "Get details for an application",
                    )
                    .with_param("app-id", NextActionParam::required()),
                ]))
        },
    )
}

#[cfg(test)]
mod tests {
    use cli_engine::CursorConfig;

    use super::command;

    #[test]
    fn opts_into_cursor_pagination_with_a_default_and_a_max_limit() {
        assert_eq!(command(false).spec.cursor, Some(CursorConfig::new(50, 100)));
    }
}
