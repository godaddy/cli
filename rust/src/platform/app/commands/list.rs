//! `gddy platform app list` — list all applications visible to the account.

use cli_engine::{
    CommandResult, CommandSpec, CursorConfig, CursorContinuation, NextActionParam,
    RuntimeCommandSpec, Tier,
};

use super::schemas::ApplicationSummary;
use crate::next_action::next_action;
use crate::pagination_token;

/// Pages through `applications(first:, after:)` with the Relay `endCursor` as
/// the `--continue` token. No supplemental `--offset` (unlike the client-side
/// "known collection" commands): a forward-only cursor has no arbitrary-jump
/// concept, since the server never hands back more than one page at a time.
pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_with_context(
        CommandSpec::new("list", "List all applications")
            .with_long(
                "List all GoDaddy developer-platform applications visible to \
                the current account, across every App Registry status. Use \
                `gddy platform app info --name <name>` to fetch full details \
                for a single application.",
            )
            .with_system("applications")
            .with_tier(Tier::Read)
            .with_default_fields("name,label,status")
            .with_output_schema::<ApplicationSummary>()
            .with_cursor(CursorConfig::new(50, 200)),
        |ctx| async move {
            let client = super::make_client(&ctx).await?;
            let page = client
                .list_applications(
                    ctx.middleware.cursor_limit,
                    ctx.middleware.continue_token.as_deref(),
                )
                .await
                .map_err(super::client_err)?;
            let (continuation, missing_cursor) =
                continuation_for(page.has_next_page, page.end_cursor);
            if missing_cursor {
                pagination_token::warn_user(
                    "More applications exist than were returned, but there is no way to \
                     fetch the rest; the list may be incomplete.",
                );
            }
            let data = serde_json::to_value(page.items).map_err(|e| {
                crate::error::GddyError::unexpected(format!("failed to encode applications: {e}"))
                    .into_cli_error()
            })?;
            Ok(CommandResult::new(data)
                .with_cursor(continuation)
                .with_next_actions(vec![
                next_action(
                    "platform app info --name <name>",
                    "Get details for a specific application",
                )
                .with_param("name", NextActionParam::required()),
                next_action(
                    "platform app init --name <name> --description <description> --url <url> --proxy-url <proxy-url> --scopes <scopes>",
                    "Initialize a new application",
                ),
            ]))
        },
    )
}

/// The continuation for one Relay page, plus whether the page is truncated
/// without a way to resume: `hasNextPage: true` with no `endCursor` is a
/// malformed response, so the listing ends there but the caller should say
/// it may be incomplete rather than letting it look complete.
fn continuation_for(has_next_page: bool, end_cursor: Option<String>) -> (CursorContinuation, bool) {
    match (has_next_page, end_cursor) {
        (true, Some(cursor)) => (CursorContinuation::more(cursor), false),
        (true, None) => (CursorContinuation::done(), true),
        (false, _) => (CursorContinuation::done(), false),
    }
}

#[cfg(test)]
mod tests {
    use cli_engine::{Cli, CliConfig, CursorConfig, Stage};

    use super::{command, continuation_for};

    #[test]
    fn a_next_page_with_a_cursor_continues() {
        let (continuation, missing) = continuation_for(true, Some("cursor-2".to_owned()));
        assert_eq!(continuation.continue_from.as_deref(), Some("cursor-2"));
        assert!(!missing);
    }

    #[test]
    fn a_next_page_without_a_cursor_ends_but_is_flagged_as_incomplete() {
        let (continuation, missing) = continuation_for(true, None);
        assert_eq!(continuation.continue_from, None);
        assert!(missing, "must warn instead of looking complete");
    }

    #[test]
    fn the_last_page_is_done_and_not_flagged() {
        for cursor in [None, Some("stale".to_owned())] {
            let (continuation, missing) = continuation_for(false, cursor);
            assert_eq!(continuation.continue_from, None);
            assert!(!missing);
        }
    }
    use crate::platform::app::client::APPLICATION_STATUSES;

    #[test]
    fn command_opts_into_cursor_pagination_with_a_default_and_a_max_limit() {
        assert_eq!(command().spec.cursor, Some(CursorConfig::new(50, 200)));
    }

    #[test]
    fn application_statuses_match_app_registry_enum() {
        assert_eq!(
            APPLICATION_STATUSES,
            ["ACTIVE", "ARCHIVED", "BLOCKED", "INACTIVE", "VERIFYING"]
        );
    }

    /// API commands must stay fail-closed: `platform app list` calls the backend,
    /// so it must require authentication. Built with **no auth provider
    /// registered**, the engine's default `AuthRequirement::Required` must reject
    /// it before the handler runs (no network call). This guards against someone
    /// mistakenly marking an API command `no_auth(true)`, which would let it run
    /// unauthenticated.
    #[tokio::test]
    async fn platform_app_list_requires_auth() {
        let cli = Cli::new(
            CliConfig::new("gddy", "GoDaddy developer CLI", "gddy")
                .with_min_stage(Stage::Experimental)
                .with_default_auth_provider("godaddy")
                .with_module(crate::platform::module()),
        );

        let output = cli
            .run(["gddy", "platform", "app", "list", "--output", "json"])
            .await;

        // The engine maps auth-resolution failures to exit code 2; together with
        // the provider-named error below this proves the command was rejected at
        // credential resolution, not by the handler hitting the network.
        const AUTH_FAILURE_EXIT: i32 = 2;
        assert_eq!(
            output.exit_code, AUTH_FAILURE_EXIT,
            "platform app list must fail closed at auth resolution, got: {}",
            output.rendered
        );
        let json: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json output");
        let message = json["error"]["message"].as_str().unwrap_or_default();
        assert!(
            message.contains("provider"),
            "expected an auth-provider resolution error, got: {}",
            output.rendered
        );
    }
}
