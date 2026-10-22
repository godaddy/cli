use cli_engine::{
    CommandResult, CommandSpec, CursorConfig, NextActionParam, RuntimeCommandSpec, Tier,
};
use serde_json::json;

use crate::hosting::common::{
    HostingDomainSummary, client_err, make_client, more_pages_warning, page_items,
};
use crate::next_action::next_action;
use crate::pagination_token::{self, warn_user};
use crate::scopes::HOSTING_DOMAIN_READ as DOMAIN_READ;

#[derive(Debug, Clone, clap::Args)]
struct DomainListArgs {
    /// Application ID.
    #[arg(long = "app-id", value_name = "APP_ID")]
    app_id: String,

    /// Jump directly to an offset in the results. Ignored when `--continue` is given.
    #[arg(long, value_name = "N")]
    offset: Option<i64>,
}

/// The API accepts no paging parameters (see `HostingClient::list_domains`),
/// so the one response is paged client-side by `--limit` / `--continue`.
pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<DomainListArgs, _, _, _>(
        CommandSpec::from_args::<DomainListArgs>("list", "List domains attached to an application")
            .with_long(
                "List domains attached to a hosting application. Shows up to 50 by \
                 default; use --limit/--continue to page through the rest.",
            )
            .with_system("hosting")
            .with_tier(Tier::Read)
            .with_scopes(&[DOMAIN_READ])
            .with_default_fields("id,hostname,verificationStatus")
            .with_output_schema::<HostingDomainSummary>()
            .with_cursor(CursorConfig::new(50, 200)),
        |ctx, args: DomainListArgs| async move {
            let app_id = args.app_id.clone();
            let client = make_client(&ctx, &[DOMAIN_READ]).await?;

            let response = client.list_domains(&app_id).await.map_err(client_err)?;
            if let Some(warning) = more_pages_warning(&response, "domains") {
                warn_user(&warning);
            }

            let continue_token = pagination_token::resolve_continue_token(
                args.offset,
                ctx.middleware.continue_token.as_deref(),
            );
            let (page, continuation) = pagination_token::paginate_known_collection(
                page_items(&response),
                ctx.middleware.cursor_limit,
                200,
                continue_token.as_deref(),
            );

            Ok(CommandResult::new(json!(page))
                .with_cursor(continuation)
                .with_next_actions(vec![
                    next_action(
                        "hosting domain attach --app-id <app-id> --hostname <hostname>",
                        "Attach a new domain",
                    )
                    .with_param("app-id", NextActionParam::value(app_id)),
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
        assert_eq!(command().spec.cursor, Some(CursorConfig::new(50, 200)));
    }
}
