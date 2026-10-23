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
    /// Jump directly to a page of results (1-based). Ignored when `--continue` is given.
    #[arg(long, value_name = "N", value_parser = clap::value_parser!(u32).range(1..))]
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
                SERVER_PAGE_SIZE_CAP as i64,
            )?;
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

/// Fetches a page from `GET /v1/email/mailboxes`. `has_more` is whether the
/// response carries a `links[rel=next]`, which the spec says is absent on the
/// last page — so a full final page correctly reports no more, and a short
/// page with a next link correctly reports more.
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
    let has_more = list.links.iter().any(|link| link.rel == "next");
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
                then.status(200).json_body(json!({
                    "items": page_of(3, 0),
                    "links": [
                        { "rel": "self", "href": "/v1/email/mailboxes?page=1&pageSize=3" },
                        { "rel": "next", "href": "/v1/email/mailboxes?page=2&pageSize=3" }
                    ]
                }));
            })
            .await;

        let client = client_for(&server);
        let (mailboxes, has_more) = fetch_mailbox_page(&client, None, None, 1, 3)
            .await
            .expect("fetch mailboxes");

        mock.assert_calls_async(1).await;
        assert_eq!(mailboxes.len(), 3);
        assert!(has_more, "a next link means another page exists");
    }

    #[tokio::test]
    async fn fetch_mailbox_page_reports_no_more_on_a_full_final_page() {
        // Exactly page-size items and no next link: the last page happens to be
        // full. An item-count heuristic would wrongly offer a page that is empty.
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/v1/email/mailboxes")
                    .query_param("page", "2")
                    .query_param("pageSize", "3");
                then.status(200).json_body(json!({
                    "items": page_of(3, 3),
                    "links": [
                        { "rel": "self", "href": "/v1/email/mailboxes?page=2&pageSize=3" },
                        { "rel": "prev", "href": "/v1/email/mailboxes?page=1&pageSize=3" }
                    ]
                }));
            })
            .await;

        let client = client_for(&server);
        let (mailboxes, has_more) = fetch_mailbox_page(&client, None, None, 2, 3)
            .await
            .expect("fetch mailboxes");

        mock.assert_calls_async(1).await;
        assert_eq!(mailboxes.len(), 3);
        assert!(!has_more, "no next link means this is the last page");
    }

    #[tokio::test]
    async fn fetch_mailbox_page_reports_more_on_a_short_page_with_a_next_link() {
        // The server may return fewer than pageSize items and still have more
        // (e.g. filtering after paging); the link, not the count, decides.
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(GET).path("/v1/email/mailboxes");
                then.status(200).json_body(json!({
                    "items": page_of(2, 0),
                    "links": [
                        { "rel": "next", "href": "/v1/email/mailboxes?page=2&pageSize=100" }
                    ]
                }));
            })
            .await;

        let client = client_for(&server);
        let (mailboxes, has_more) =
            fetch_mailbox_page(&client, None, None, 1, SERVER_PAGE_SIZE_CAP)
                .await
                .expect("fetch mailboxes");

        mock.assert_calls_async(1).await;
        assert_eq!(mailboxes.len(), 2);
        assert!(has_more, "a next link means more even on a short page");
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
        assert!(!has_more, "no next link means the list is exhausted");
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
