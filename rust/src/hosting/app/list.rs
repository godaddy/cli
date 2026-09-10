use cli_engine::{CommandResult, CommandSpec, NextActionParam, RuntimeCommandSpec, Tier};
use serde_json::{Value, json};

use crate::hosting::common::{
    HostingAppSummary, client_err, make_client, next_page_token, parse_app_type,
};
use crate::next_action::next_action;
use crate::scopes::HOSTING_APPLICATION_READ as APP_READ;

#[derive(Debug, Clone, clap::Args)]
struct AppListArgs {
    /// Application type (NODEJS).
    #[arg(long = "app-type", value_name = "TYPE", value_parser = parse_app_type)]
    app_type: String,

    /// Maximum number of applications to return. Omit to return all.
    #[arg(long, value_name = "N", value_parser = clap::value_parser!(u32).range(1..))]
    limit: Option<u32>,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<AppListArgs, _, _, _>(
        CommandSpec::from_args::<AppListArgs>("list", "List hosting applications")
            .with_long(
                "List all hosting applications of a given type. Results are autopaginated — \
                 all pages are fetched and combined. Use --limit to cap the total returned.\n\
                 \n\
                 --app-type is required. Currently supported: NODEJS.",
            )
            .with_system("hosting")
            .with_tier(Tier::Read)
            .with_scopes(&[APP_READ])
            .with_default_fields("id,name,status")
            .with_output_schema::<HostingAppSummary>(),
        |ctx, args: AppListArgs| async move {
            let app_type = args.app_type;
            let limit = args.limit;
            let client = make_client(&ctx, &[APP_READ]).await?;

            let mut all_items: Vec<Value> = Vec::new();
            let mut page_token: Option<String> = None;

            loop {
                let page_limit = limit.map(|cap| {
                    let remaining = cap.saturating_sub(all_items.len() as u32);
                    remaining.min(100)
                });

                let response = client
                    .list_apps(&app_type, page_token.as_deref(), page_limit)
                    .await
                    .map_err(client_err)?;

                if let Some(items) = response.get("items").and_then(|v| v.as_array()) {
                    all_items.extend(items.iter().cloned());
                }

                if limit.is_some_and(|cap| all_items.len() >= cap as usize) {
                    if let Some(cap) = limit {
                        all_items.truncate(cap as usize);
                    }
                    break;
                }

                match next_page_token(&response) {
                    Some(token) => page_token = Some(token),
                    None => break,
                }
            }

            Ok(CommandResult::new(json!(all_items)).with_next_actions(vec![
                next_action(
                    "hosting app get --app-id <app-id>",
                    "Get details for an application",
                )
                .with_param("app-id", NextActionParam::required()),
            ]))
        },
    )
}
