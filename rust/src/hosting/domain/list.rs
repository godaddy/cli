use cli_engine::{
    CommandResult, CommandSpec, CursorConfig, NextActionParam, RuntimeCommandSpec, Tier,
};
use serde_json::json;

use crate::hosting::common::{
    HostingDomainSummary, client_err, incomplete_list_warning, make_client,
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

/// The service doesn't paginate this listing yet (see
/// `HostingClient::list_domains`), so every domain is gathered — following the
/// service's `next` links if it ever sends them — and `--limit` / `--continue`
/// page through that whole list client-side. Once the service accepts paging
/// parameters this moves to its own cursor.
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

            let continue_token = pagination_token::resolve_continue_token(
                args.offset,
                ctx.middleware.continue_token.as_deref(),
            );
            // Read only as many pages as the window needs. A `--filter` runs
            // after collection, so with one the whole list is needed.
            let want = ctx.middleware.filter.is_empty().then(|| {
                pagination_token::known_collection_need(
                    continue_token.as_deref(),
                    ctx.middleware.cursor_limit,
                    200,
                )
            });
            let listed = client
                .list_domains(&app_id, want)
                .await
                .map_err(client_err)?;
            if listed.incomplete {
                warn_user(&incomplete_list_warning("domains"));
            }

            let (page, continuation) = pagination_token::paginate_known_collection(
                pagination_token::filter_known_collection(listed.items, &ctx.middleware.filter)?,
                ctx.middleware.cursor_limit,
                200,
                continue_token.as_deref(),
            );

            // Pages past the window were deliberately not read, so the length of
            // what was read says nothing about the total.
            let continuation = if listed.stopped_early {
                pagination_token::forget_totals(continuation)
            } else {
                continuation
            };

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
