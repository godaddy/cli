use cli_engine::{
    CommandResult, CommandSpec, NextActionParam, PaginationConfig, RuntimeCommandSpec, Tier,
};
use serde_json::{Value, json};

use crate::email::client::{ClientError, EmailClient};
use crate::email::{client_err, make_client};
use crate::next_action::next_action;
use crate::scopes::EMAIL_READ;

/// panel-v3-api's own page-size cap for `GET /v1/email/mailboxes`. See the
/// design doc's pagination decision.
const SERVER_PAGE_SIZE_CAP: usize = 100;

#[derive(Debug, Clone, clap::Args)]
struct ListArgs {
    /// Only mailboxes with this status, e.g. ACTIVE.
    #[arg(long, value_name = "STATUS")]
    status: Option<String>,
    /// Comma-separated list of fields to include in the response.
    #[arg(long, value_name = "FIELDS")]
    fields: Option<String>,
}

/// Translates `--limit`/`--offset` into the panel API's native `page`/`pageSize`
/// query params via [`fetch_mailboxes`], fetching only as many leading pages as
/// needed to cover the requested window (see
/// `docs/proposals/email-management-cli.md`'s pagination decision) instead of the
/// full collection. Returns a bare JSON array so the engine's `--limit`/`--offset`
/// pipeline (`PaginationConfig`) can slice it to the exact window.
pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<ListArgs, _, _, _>(
        CommandSpec::from_args::<ListArgs>("list", "List your Email mailboxes")
            .with_system("email")
            .with_tier(Tier::Read)
            .with_scopes(&[EMAIL_READ])
            .with_pagination(PaginationConfig {
                default_limit: 25,
                max_limit: 500,
            }),
        |ctx, args: ListArgs| async move {
            let client = make_client(&ctx, &[EMAIL_READ]).await?;
            let mailboxes = fetch_mailboxes(
                &client,
                args.status.as_deref(),
                args.fields.as_deref(),
                ctx.middleware.limit,
                ctx.middleware.offset,
            )
            .await
            .map_err(client_err)?;
            Ok(CommandResult::new(json!(mailboxes)).with_next_actions(vec![
                next_action("email get <mailbox-id>", "Get a mailbox by ID")
                    .with_param("mailbox-id", NextActionParam::required()),
            ]))
        },
    )
}

/// Fetches only as many leading pages as needed to cover `[offset, offset +
/// limit)`, plus one extra item beyond that window (when the server isn't
/// already exhausted) so the engine's pagination pipeline reports an accurate
/// `has_more`. `limit <= 0` means unlimited (`PaginationConfig` convention) and
/// fetches every page. Trade-off: the returned `total` reflects only what was
/// fetched, not the panel API's true mailbox count, whenever more exist beyond
/// the requested window — computing an exact `total` would require fetching
/// everything, which this early-stop is meant to avoid.
async fn fetch_mailboxes(
    client: &EmailClient,
    status: Option<&str>,
    fields: Option<&str>,
    limit: i64,
    offset: i64,
) -> Result<Vec<Value>, ClientError> {
    let target = if limit > 0 {
        let extra = offset.saturating_add(limit).saturating_add(1);
        Some(usize::try_from(extra).unwrap_or(usize::MAX))
    } else {
        None
    };

    let mut mailboxes = Vec::new();
    let mut page: u32 = 1;
    loop {
        let mut query: Vec<(&str, String)> = vec![
            ("page", page.to_string()),
            ("pageSize", SERVER_PAGE_SIZE_CAP.to_string()),
        ];
        if let Some(status) = status {
            query.push(("status", status.to_owned()));
        }
        if let Some(fields) = fields {
            query.push(("field", fields.to_owned()));
        }

        let data = client.list_mailboxes(&query).await?;
        let page_items = data
            .get("items")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let got = page_items.len();
        mailboxes.extend(page_items);

        let exhausted = got < SERVER_PAGE_SIZE_CAP;
        let covered = match target {
            Some(t) => mailboxes.len() >= t,
            None => false,
        };
        if exhausted || covered {
            break;
        }
        page += 1;
    }

    Ok(mailboxes)
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
    fn opts_into_pagination_with_a_default_and_a_max_limit() {
        assert_eq!(
            command().spec.pagination,
            Some(PaginationConfig {
                default_limit: 25,
                max_limit: 500,
            })
        );
    }

    fn page_of(n: usize, start: usize) -> Vec<Value> {
        (start..start + n)
            .map(|i| json!({ "mailboxId": format!("mbx-{i}"), "status": "ACTIVE" }))
            .collect()
    }

    #[tokio::test]
    async fn stops_after_one_short_page_even_when_target_is_not_yet_covered() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/v1/email/mailboxes")
                    .query_param("page", "1");
                then.status(200)
                    .json_body(json!({ "items": page_of(3, 0) }));
            })
            .await;

        let client = EmailClient::new(server.base_url(), "test-token");
        let mailboxes = fetch_mailboxes(&client, None, None, 2, 0)
            .await
            .expect("fetch mailboxes");

        mock.assert_calls_async(1).await;
        assert_eq!(mailboxes.len(), 3);
    }

    #[tokio::test]
    async fn fetches_a_second_page_when_offset_and_limit_cross_a_page_boundary() {
        let server = MockServer::start_async().await;
        let page1 = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/v1/email/mailboxes")
                    .query_param("page", "1");
                then.status(200)
                    .json_body(json!({ "items": page_of(SERVER_PAGE_SIZE_CAP, 0) }));
            })
            .await;
        let page2 = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/v1/email/mailboxes")
                    .query_param("page", "2");
                then.status(200)
                    .json_body(json!({ "items": page_of(10, SERVER_PAGE_SIZE_CAP) }));
            })
            .await;

        let client = EmailClient::new(server.base_url(), "test-token");
        let mailboxes = fetch_mailboxes(&client, None, None, 5, 99)
            .await
            .expect("fetch mailboxes");

        page1.assert_calls_async(1).await;
        page2.assert_calls_async(1).await;
        assert_eq!(mailboxes.len(), SERVER_PAGE_SIZE_CAP + 10);
    }

    #[tokio::test]
    async fn unlimited_limit_fetches_until_a_short_page_is_seen() {
        let server = MockServer::start_async().await;
        let page1 = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/v1/email/mailboxes")
                    .query_param("page", "1");
                then.status(200)
                    .json_body(json!({ "items": page_of(SERVER_PAGE_SIZE_CAP, 0) }));
            })
            .await;
        let page2 = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/v1/email/mailboxes")
                    .query_param("page", "2");
                then.status(200)
                    .json_body(json!({ "items": page_of(5, SERVER_PAGE_SIZE_CAP) }));
            })
            .await;

        let client = EmailClient::new(server.base_url(), "test-token");
        let mailboxes = fetch_mailboxes(&client, None, None, 0, 0)
            .await
            .expect("fetch mailboxes");

        page1.assert_calls_async(1).await;
        page2.assert_calls_async(1).await;
        assert_eq!(mailboxes.len(), SERVER_PAGE_SIZE_CAP + 5);
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

        let client = EmailClient::new(server.base_url(), "test-token");
        fetch_mailboxes(&client, Some("ACTIVE"), Some("mailboxId,status"), 10, 0)
            .await
            .expect("fetch mailboxes");

        mock.assert_async().await;
    }
}
