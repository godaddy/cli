use cli_engine::{
    CliCoreError, CommandResult, CommandSpec, CursorConfig, CursorContinuation, NextActionParam,
    RuntimeCommandSpec, Tier,
};
use email_client::types;

use crate::email::client::{ClientError, EmailClient};
use crate::email::{client_err, make_client};
use crate::next_action::next_action;
use crate::pagination_token;
use crate::scopes::EMAIL_READ;

/// panel-v3-api's page-size cap for `GET /v1/email/mailboxes`
const SERVER_PAGE_SIZE_CAP: usize = 100;

#[derive(Debug, Clone, clap::Args)]
struct ListArgs {
    /// Only mailboxes with this status, e.g. ACTIVE.
    #[arg(long, value_name = "STATUS")]
    status: Option<String>,
    /// Comma-separated list of fields to include in the response.
    #[arg(long, value_name = "FIELDS")]
    fields: Option<String>,
    /// Jump directly to a page of results (1-based).
    #[arg(long, value_name = "N", conflicts_with = "continue")]
    page: Option<u32>,
}

/// Translates `--limit`/`--continue` into the panel API's native
/// `page`/`pageSize` query params via [`fetch_mailbox_page`]
pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<ListArgs, _, _, _>(
        CommandSpec::from_args::<ListArgs>("list", "List your Email mailboxes")
            .with_system("email")
            .with_tier(Tier::Read)
            .with_scopes(&[EMAIL_READ])
            .with_cursor(CursorConfig::new(25, SERVER_PAGE_SIZE_CAP as i64)),
        |ctx, args: ListArgs| async move {
            let client = make_client(&ctx, &[EMAIL_READ]).await?;
            let (page, page_size) = pagination_token::resolve_page_request(
                args.page,
                ctx.middleware.continue_token.as_deref(),
                1u32,
                ctx.middleware.cursor_limit,
            );
            let page_size = usize::try_from(page_size).unwrap_or(SERVER_PAGE_SIZE_CAP);
            let (mailboxes, has_more) = fetch_mailbox_page(
                &client,
                args.status.as_deref(),
                args.fields.as_deref(),
                page,
                page_size,
            )
            .await
            .map_err(client_err)?;
            let continuation = if has_more {
                CursorContinuation::more(pagination_token::build(&[
                    ("limit", &page_size.to_string()),
                    ("page", &(page + 1).to_string()),
                ]))
            } else {
                CursorContinuation::done()
            };
            let continuation =
                continuation.with_limit(i64::try_from(page_size).unwrap_or(i64::MAX));
            let value = serde_json::to_value(&mailboxes).map_err(|e| {
                CliCoreError::message(format!("failed to serialize mailboxes: {e}"))
            })?;
            Ok(CommandResult::new(value)
                .with_cursor(continuation)
                .with_next_actions(vec![
                    next_action("email get <mailbox-id>", "Get a mailbox by ID")
                        .with_param("mailbox-id", NextActionParam::required()),
                ]))
        },
    )
}

/// Fetches a page from `GET /v1/email/mailboxes`. `has_more` is a
/// short-page heuristic (a full page might still be the exact last one) —
/// this API never reports a true total, so "short page = done" is the best
/// signal available.
async fn fetch_mailbox_page(
    client: &EmailClient,
    status: Option<&str>,
    fields: Option<&str>,
    page: u32,
    page_size: usize,
) -> Result<(Vec<types::Mailbox>, bool), ClientError> {
    let list = client
        .list_mailboxes(
            status,
            page,
            u32::try_from(page_size).unwrap_or(u32::MAX),
            fields,
        )
        .await?;
    let has_more = list.items.len() >= page_size;
    Ok((list.items, has_more))
}

#[cfg(test)]
mod tests {
    use httpmock::prelude::*;
    use serde_json::json;

    use super::*;

    #[test]
    fn is_a_read_tier_command_scoped_to_email_read() {
        let spec = command().spec;
        assert_eq!(spec.tier, Some(Tier::Read));
        assert_eq!(spec.metadata().scopes, vec![EMAIL_READ.to_string()]);
    }

    #[test]
    fn opts_into_cursor_pagination_with_a_default_and_a_max_limit() {
        assert_eq!(command().spec.cursor, Some(CursorConfig::new(25, 100)));
    }

    fn page_of(n: usize, start: usize) -> Vec<serde_json::Value> {
        (start..start + n)
            .map(|i| json!({ "mailboxId": format!("mbx-{i}"), "status": "ACTIVE" }))
            .collect()
    }

    fn client_for(server: &MockServer) -> EmailClient {
        EmailClient::new(server.base_url(), "test-token").expect("client should build")
    }

    #[tokio::test]
    async fn fetch_mailbox_page_reports_has_more_when_the_page_is_full() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/v1/email/mailboxes")
                    .query_param("page", "1")
                    .query_param("pageSize", "3");
                then.status(200)
                    .json_body(json!({ "items": page_of(3, 0) }));
            })
            .await;

        let client = client_for(&server);
        let (mailboxes, has_more) = fetch_mailbox_page(&client, None, None, 1, 3)
            .await
            .expect("fetch mailboxes");

        mock.assert_calls_async(1).await;
        assert_eq!(mailboxes.len(), 3);
        assert!(has_more, "a full page is treated as possibly having more");
    }

    #[tokio::test]
    async fn fetch_mailbox_page_reports_no_more_on_a_short_page() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/v1/email/mailboxes")
                    .query_param("page", "2")
                    .query_param("pageSize", "100");
                then.status(200)
                    .json_body(json!({ "items": page_of(10, SERVER_PAGE_SIZE_CAP) }));
            })
            .await;

        let client = client_for(&server);
        let (mailboxes, has_more) =
            fetch_mailbox_page(&client, None, None, 2, SERVER_PAGE_SIZE_CAP)
                .await
                .expect("fetch mailboxes");

        mock.assert_calls_async(1).await;
        assert_eq!(mailboxes.len(), 10);
        assert!(!has_more, "a short page means the list is exhausted");
    }

    #[tokio::test]
    async fn forwards_status_and_fields_query_params() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/v1/email/mailboxes")
                    .query_param("status", "ACTIVE")
                    .query_param("field", "mailboxId,status");
                then.status(200)
                    .json_body(json!({ "items": page_of(1, 0) }));
            })
            .await;

        let client = client_for(&server);
        fetch_mailbox_page(&client, Some("ACTIVE"), Some("mailboxId,status"), 1, 100)
            .await
            .expect("fetch mailboxes");

        mock.assert_async().await;
    }
}
