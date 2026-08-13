//! `gddy domain list` — list the domains in the account (v3).

use cli_engine::{
    CliCoreError, CommandResult, CommandSpec, NextActionParam, PaginationConfig, Result,
    RuntimeCommandSpec, Tier,
};
use serde_json::json;

use domains_client::types;

use super::common::{api_error, comma_joined, make_client};
use crate::next_action::next_action;
use crate::scopes::DOMAINS_READ;

/// Lifecycle groups that make up GoDaddy's default "visible" domain view —
/// every group except `TERMINAL` (cancelled, confiscated, transferred out,
/// deleted-redeemable, etc.). `DomainLifecycleGroup` has no generated enum
/// (the spec declares it a bare string), so these are validated server-side.
const DEFAULT_VISIBLE_GROUPS: [&str; 3] = ["PENDING", "REGISTERED", "PENDING_TERMINAL"];

/// v3's max `pageSize` (the spec's `maximum: 200`). Requesting it on every
/// page minimizes round trips against an API observed to rate-limit as
/// tightly as 5 requests per period.
const MAX_PAGE_SIZE: u64 = 200;

/// Defensive cap on pages fetched for one invocation. No real account should
/// ever approach `MAX_PAGE_SIZE * MAX_PAGES` (10,000) domains; hitting this
/// is treated as a bug (a malformed or looping `next` link) rather than
/// silently returned as if it were the complete list.
const MAX_PAGES: usize = 50;

/// Validate each `--status` value case-insensitively against the generated
/// `DomainStatus` enum (e.g. `ACTIVE`), returning the canonical uppercase
/// wire form. The `statuses` query parameter is typed as plain strings (not
/// `$ref: DomainStatus`) purely so it can be comma-joined client-side (see
/// `comma_joined`) — this still rejects unknown values before the request
/// goes out, same as if the array held real `DomainStatus`es.
fn parse_statuses(raw: &[String]) -> Result<Vec<String>> {
    raw.iter()
        .map(|s| {
            types::DomainStatus::try_from(s.to_uppercase().as_str())
                .map(|_| s.to_uppercase())
                .map_err(|_| CliCoreError::message(format!("invalid --status {s:?}")))
        })
        .collect()
}

/// Whether the request should be scoped to the default "visible" lifecycle
/// groups — GoDaddy's default view that hides cancelled/confiscated/other
/// terminal domains. Skipped when the caller passed an explicit `--status`
/// filter or asked to see hidden domains via `--show-hidden`.
fn wants_visible_only(statuses: &[String], show_hidden: bool) -> bool {
    statuses.is_empty() && !show_hidden
}

#[derive(Debug, Clone, clap::Args)]
struct ListArgs {
    /// Only domains with this status, e.g. ACTIVE (repeatable).
    #[arg(long, value_name = "STATUS")]
    status: Vec<String>,

    /// Include domains hidden by default, e.g. cancelled or confiscated.
    #[arg(long)]
    show_hidden: bool,
}

/// Extract `pageToken`/`pageTokenDirection` from a `DomainCollection`'s
/// `rel=next` link, if present. `None` (whether there's genuinely no next
/// page, or a `next` link is present but missing an `href`/`pageToken` this
/// CLI can parse) means "stop" — a domain list that ends one page short of
/// complete is far less harmful than one that loops or errors on a link
/// shape it doesn't recognize.
fn next_page_token(
    links: &[types::LinkDescription],
) -> Option<(String, Option<types::ListDomainsPageTokenDirection>)> {
    let href = links
        .iter()
        .find(|l| l.rel.as_deref() == Some("next"))?
        .href
        .as_deref()?;
    let url = url::Url::parse(href).ok()?;
    let mut token = None;
    let mut direction = None;
    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "pageToken" => token = Some(value.into_owned()),
            "pageTokenDirection" => {
                direction = types::ListDomainsPageTokenDirection::try_from(value.as_ref()).ok();
            }
            _ => {}
        }
    }
    token.map(|token| (token, direction))
}

/// Fetch every domain matching `statuses`/`visible_only`, following v3's
/// `links[rel=next]` cursor until the API reports no further page — or until
/// `stop_at` items have been accumulated, when an explicit `--limit`/
/// `--offset` window needs no more than that many (cli-engine's own
/// pagination pipeline slices the exact window from whatever this returns;
/// fetching further would be wasted work against a tightly rate-limited
/// API). `stop_at: None` fetches everything, matching this command's
/// pre-v3 behavior of returning every domain when unflagged.
async fn fetch_domains(
    client: &domains_client::Client,
    statuses: &[String],
    visible_only: bool,
    stop_at: Option<usize>,
) -> std::result::Result<Vec<types::Domain>, domains_client::Error<()>> {
    let page_size = std::num::NonZeroU64::new(MAX_PAGE_SIZE).expect("nonzero constant");
    let mut items = Vec::new();
    let mut page_token = None;
    for _ in 0..MAX_PAGES {
        let mut req = client.list_domains().page_size(page_size);
        if !statuses.is_empty() {
            // `statuses` is `style: form, explode: false` — one
            // comma-joined value, not repeated `statuses=` pairs
            // (progenitor always seq-serializes a `Vec` as repeated pairs
            // regardless of the spec's `explode` setting; see
            // `comma_joined`'s doc comment / DEVEX-882).
            req = req.statuses(comma_joined(statuses.to_vec()));
        } else if visible_only {
            req = req.lifecycle_groups(
                comma_joined(
                    DEFAULT_VISIBLE_GROUPS
                        .into_iter()
                        .map(str::to_string)
                        .collect(),
                )
                .into_iter()
                .map(types::DomainLifecycleGroup::from)
                .collect::<Vec<_>>(),
            );
        }
        if let Some((token, direction)) = page_token.take() {
            req = req.page_token(token);
            if let Some(direction) = direction {
                req = req.page_token_direction(direction);
            }
        }
        let collection = req.send().await?.into_inner();
        items.extend(collection.items.unwrap_or_default());
        if stop_at.is_some_and(|n| items.len() >= n) {
            break;
        }
        page_token = collection.links.and_then(|links| next_page_token(&links));
        if page_token.is_none() {
            break;
        }
    }
    Ok(items)
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<ListArgs, _, _, _>(
        CommandSpec::from_args::<ListArgs>("list", "List the domains in your account")
            .with_long(
                "List the domains registered to your account. Shows domain, status, \
                 expiry, and auto-renew by default; use --fields to pick columns. Hides \
                 domains that are in a terminal status unless --show-hidden \
                 is passed or specific status values are specified with --status \
                 (repeatable).",
            )
            .with_system("domain")
            .with_tier(Tier::Read)
            .with_default_fields("domain,status,expiresAt,autoRenew")
            .with_json_schema::<types::Domain>()
            .with_scopes(&[DOMAINS_READ])
            .with_pagination(PaginationConfig {
                max_limit: 500,
                ..Default::default()
            }),
        |ctx, args: ListArgs| async move {
            let debug = !ctx.middleware.debug.is_empty();
            let statuses = parse_statuses(&args.status)?;
            let show_hidden = args.show_hidden;
            let visible_only = wants_visible_only(&statuses, show_hidden);
            let client = make_client(&ctx).await?;
            // An explicit `--limit`/`--offset` window needs no more than
            // `offset + limit` domains; cli-engine's pagination pipeline
            // (`ctx.middleware.limit`/`.offset`, populated from those flags)
            // slices the exact window from whatever this returns, so
            // fetching further would be wasted requests against a tightly
            // rate-limited API. Unflagged (`limit == 0`, the "unlimited"
            // sentinel) fetches every domain, matching this command's
            // pre-v3 behavior.
            let limit = ctx.middleware.limit;
            let stop_at = (limit > 0).then(|| {
                usize::try_from(ctx.middleware.offset.max(0) + limit).unwrap_or(usize::MAX)
            });
            let items = match fetch_domains(&client, &statuses, visible_only, stop_at).await {
                Ok(items) => items,
                Err(e) => return Err(api_error("listing domains", debug, e).await),
            };
            let domains: Vec<serde_json::Value> = items
                .iter()
                .map(serde_json::to_value)
                .collect::<std::result::Result<_, _>>()
                .map_err(|e| {
                    CliCoreError::message(format!("failed to serialize domain list: {e}"))
                })?;
            Ok(CommandResult::new(json!(domains)).with_next_actions(vec![
                next_action("dns list <domain>", "View a domain's DNS records")
                    .with_param("domain", NextActionParam::required()),
            ]))
        },
    )
}

#[cfg(test)]
mod tests {
    use super::{command, fetch_domains, next_page_token, parse_statuses, wants_visible_only};
    use cli_engine::PaginationConfig;
    use domains_client::types;

    /// Regression pin: `domain list` opts into pagination with no forced
    /// `default_limit` (an unflagged invocation must keep returning every
    /// domain, exactly like before this feature existed) but a `max_limit`
    /// so `--limit` can't be pointed at an absurd value.
    #[test]
    fn opts_into_pagination_with_no_default_and_a_max_limit() {
        assert_eq!(
            command().spec.pagination,
            Some(PaginationConfig {
                max_limit: 500,
                ..Default::default()
            })
        );
    }

    #[test]
    fn parse_statuses_is_case_insensitive_and_validates() {
        let parsed = parse_statuses(&["active".to_string(), "CANCELLED".to_string()])
            .expect("valid statuses");
        assert_eq!(parsed, vec!["ACTIVE".to_string(), "CANCELLED".to_string()]);
        assert!(parse_statuses(&[]).expect("empty ok").is_empty());
        let err = parse_statuses(&["bogus".to_string()]).expect_err("should reject");
        assert!(err.to_string().contains("invalid --status"), "{err}");
    }

    #[test]
    fn wants_visible_only_defaults_true_but_yields_to_status_or_show_hidden() {
        assert!(wants_visible_only(&[], false));
        assert!(!wants_visible_only(&[], true));
        assert!(!wants_visible_only(&["CANCELLED".to_string()], false));
    }

    #[tokio::test]
    async fn statuses_are_sent_as_a_single_comma_joined_query_param() {
        // Regression for DEVEX-882 (mirrors `agreements.rs`'s equivalent
        // test): v3's `statuses` query param is `style: form, explode:
        // false` — one comma-joined value. progenitor's generated
        // `statuses()` setter always seq-serializes a `Vec` as repeated
        // `statuses=` pairs, which the live API rejects with
        // `MISMATCH_FORMAT` (confirmed against a real test-environment
        // server) — multiple `--status` values must be joined first.
        let server = httpmock::MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::GET)
                    .path("/v3/domains/domain-names")
                    .query_param("statuses", "ACTIVE,EXPIRED");
                then.status(200)
                    .json_body(serde_json::json!({ "items": [] }));
            })
            .await;
        let client =
            domains_client::client_with_auth(&server.base_url(), "Bearer tok", "test", "req-1")
                .expect("build client");

        let statuses =
            super::super::common::comma_joined(vec!["ACTIVE".to_string(), "EXPIRED".to_string()]);
        client
            .list_domains()
            .statuses(statuses)
            .send()
            .await
            .expect("request succeeds");

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn default_visible_only_lifecycle_groups_are_comma_joined() {
        // Same DEVEX-882 class of bug applies to `lifecycleGroups` — the
        // no-flags default view (hide non-visible/terminal domains).
        let server = httpmock::MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::GET)
                    .path("/v3/domains/domain-names")
                    .query_param("lifecycleGroups", "PENDING,REGISTERED,PENDING_TERMINAL");
                then.status(200)
                    .json_body(serde_json::json!({ "items": [] }));
            })
            .await;
        let client =
            domains_client::client_with_auth(&server.base_url(), "Bearer tok", "test", "req-1")
                .expect("build client");

        let groups = super::super::common::comma_joined(
            super::DEFAULT_VISIBLE_GROUPS
                .into_iter()
                .map(str::to_string)
                .collect(),
        )
        .into_iter()
        .map(types::DomainLifecycleGroup::from)
        .collect::<Vec<_>>();
        client
            .list_domains()
            .lifecycle_groups(groups)
            .send()
            .await
            .expect("request succeeds");

        mock.assert_async().await;
    }

    fn next_link(href: &str) -> Vec<types::LinkDescription> {
        vec![types::LinkDescription {
            href: Some(href.to_string()),
            rel: Some("next".to_string()),
            ..Default::default()
        }]
    }

    #[test]
    fn next_page_token_parses_token_and_direction_from_the_next_link() {
        let links = next_link(
            "https://api.example.com/v3/domains/domain-names?pageToken=abc123&pageTokenDirection=forward",
        );
        let (token, direction) = next_page_token(&links).expect("next link present");
        assert_eq!(token, "abc123");
        assert_eq!(
            direction,
            Some(types::ListDomainsPageTokenDirection::Forward)
        );
    }

    #[test]
    fn next_page_token_is_none_without_a_next_rel() {
        let mut links = next_link("https://api.example.com/v3/domains/domain-names?pageToken=abc");
        links[0].rel = Some("self".to_string());
        assert!(next_page_token(&links).is_none());
    }

    #[test]
    fn next_page_token_is_none_when_the_next_link_has_no_page_token() {
        let links = next_link("https://api.example.com/v3/domains/domain-names");
        assert!(next_page_token(&links).is_none());
    }

    #[tokio::test]
    async fn fetch_domains_follows_the_next_link_across_pages() {
        // Regression: `listDomains` is cursor-paginated (`DomainCollection`
        // with `links[rel=next]`), but a single `send()` only returns the
        // first page — an account with more domains than one page would get
        // silently truncated results. This proves `fetch_domains` follows
        // `next` until it's exhausted rather than stopping after page one.
        let server = httpmock::MockServer::start_async().await;
        let page1 = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::GET)
                    .path("/v3/domains/domain-names")
                    .query_param_missing("pageToken");
                then.status(200).json_body(serde_json::json!({
                    "items": [{"domain": "a.com"}],
                    "links": [{
                        "rel": "next",
                        "href": "https://ignored.example.com/v3/domains/domain-names?pageToken=page-2",
                    }],
                }));
            })
            .await;
        let page2 = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::GET)
                    .path("/v3/domains/domain-names")
                    .query_param("pageToken", "page-2");
                then.status(200).json_body(serde_json::json!({
                    "items": [{"domain": "b.com"}],
                }));
            })
            .await;
        let client =
            domains_client::client_with_auth(&server.base_url(), "Bearer tok", "test", "req-1")
                .expect("build client");

        let items = fetch_domains(&client, &[], false, None)
            .await
            .expect("fetch succeeds");

        assert_eq!(
            items
                .iter()
                .filter_map(|d| d.domain.clone())
                .collect::<Vec<_>>(),
            vec!["a.com".to_string(), "b.com".to_string()]
        );
        page1.assert_async().await;
        page2.assert_async().await;
    }

    #[tokio::test]
    async fn fetch_domains_stops_once_stop_at_is_satisfied_without_fetching_the_next_page() {
        // The `--limit`/`--offset` window is satisfied by page one alone, so
        // `fetch_domains` must not spend a second request (and a second hit
        // against this API's tight rate limit) fetching page two.
        let server = httpmock::MockServer::start_async().await;
        let page1 = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::GET)
                    .path("/v3/domains/domain-names");
                then.status(200).json_body(serde_json::json!({
                    "items": [{"domain": "a.com"}, {"domain": "b.com"}],
                    "links": [{
                        "rel": "next",
                        "href": "https://ignored.example.com/v3/domains/domain-names?pageToken=page-2",
                    }],
                }));
            })
            .await;
        let client =
            domains_client::client_with_auth(&server.base_url(), "Bearer tok", "test", "req-1")
                .expect("build client");

        let items = fetch_domains(&client, &[], false, Some(2))
            .await
            .expect("fetch succeeds");

        assert_eq!(items.len(), 2);
        assert_eq!(
            page1.calls_async().await,
            1,
            "must not fetch beyond the satisfied stop_at window"
        );
    }
}
