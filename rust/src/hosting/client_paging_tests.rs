//! Tests for the hosting list commands' multi-page handling: following `next`
//! links on the domain and subscription listings, stopping early, and keeping
//! followed pages out of the transport trace.

use httpmock::prelude::*;
use serde_json::json;

use super::*;

fn client(base_url: &str) -> HostingClient {
    HostingClient::new(base_url, "test-token")
}

#[tokio::test]
async fn list_domains_without_a_cursor_sends_no_paging_parameters() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/v1/hosting/apps/app-1/domains")
                .query_param_missing("pageToken")
                .query_param_missing("pageSize")
                .query_param_missing("page");
            then.status(200)
                .json_body(json!({ "items": [], "links": [] }));
        })
        .await;

    client(&server.base_url())
        .list_domains("app-1", None)
        .await
        .expect("list domains");

    mock.assert_async().await;
}

fn domains_first_page(server: &MockServer, body: Value) -> httpmock::Mock<'_> {
    server.mock(|when, then| {
        when.method(GET)
            .path("/v1/hosting/apps/app-1/domains")
            .query_param_missing("pageToken");
        then.status(200).json_body(body);
    })
}

fn domain_ids(listed: &CombinedPages) -> Vec<&str> {
    listed
        .items
        .iter()
        .map(|item| item["domainId"].as_str().unwrap_or(""))
        .collect()
}

#[tokio::test]
async fn list_domains_combines_pages_by_replaying_the_next_links_query() {
    let server = MockServer::start_async().await;
    // The link's query is already percent-encoded (`%2B` is a plus, `%3D` an
    // equals sign in the opaque token); replaying it must not encode it a second
    // time. The path and bearer token are ours, not taken from the link.
    let first = domains_first_page(
        &server,
        json!({
            "items": [{ "domainId": "d1" }],
            "links": [{
                "rel": "next",
                "href": "/v1/hosting/apps/app-1/domains?pageToken=a%2Bb%3D%3D&pageSize=25"
            }]
        }),
    );
    let second = server.mock(|when, then| {
        when.method(GET)
            .path("/v1/hosting/apps/app-1/domains")
            .query_param("pageToken", "a+b==")
            .query_param("pageSize", "25")
            .header("authorization", "Bearer test-token");
        // `noise` is not in the spec; it must not survive on later pages only.
        then.status(200).json_body(json!({
            "items": [{ "domainId": "d2", "noise": 1 }, { "domainId": "d3" }],
            "links": [{ "rel": "self", "href": "/v1/hosting/apps/app-1/domains" }]
        }));
    });

    let listed = client(&server.base_url())
        .list_domains("app-1", None)
        .await
        .expect("list domains");

    first.assert_calls(1);
    second.assert_calls(1);
    assert_eq!(domain_ids(&listed), ["d1", "d2", "d3"]);
    assert!(!listed.incomplete);
    assert!(
        listed.items.iter().all(|i| i.get("noise").is_none()),
        "{:?}",
        listed.items
    );
}

#[tokio::test]
async fn list_domains_stops_reading_pages_once_the_wanted_items_are_held() {
    let server = MockServer::start_async().await;
    domains_first_page(
        &server,
        json!({
            "items": [{ "domainId": "d1" }, { "domainId": "d2" }],
            "links": [{ "rel": "next", "href": "/v1/hosting/apps/app-1/domains?pageToken=t2" }]
        }),
    );
    let second = server.mock(|when, then| {
        when.method(GET)
            .path("/v1/hosting/apps/app-1/domains")
            .query_param("pageToken", "t2");
        then.status(200)
            .json_body(json!({ "items": [{ "domainId": "d3" }], "links": [] }));
    });

    let listed = client(&server.base_url())
        .list_domains("app-1", Some(2))
        .await
        .expect("list domains");

    second.assert_calls(0);
    assert_eq!(domain_ids(&listed), ["d1", "d2"]);
    assert!(listed.is_partial(), "a skipped page means no total");
    assert!(listed.stopped_early, "the service still had a page");
    assert!(!listed.incomplete, "stopping on purpose is not a problem");
}

#[tokio::test]
async fn list_domains_reads_on_while_the_wanted_items_are_not_yet_held() {
    let server = MockServer::start_async().await;
    domains_first_page(
        &server,
        json!({
            "items": [{ "domainId": "d1" }, { "domainId": "d2" }],
            "links": [{ "rel": "next", "href": "/v1/hosting/apps/app-1/domains?pageToken=t2" }]
        }),
    );
    let second = server.mock(|when, then| {
        when.method(GET)
            .path("/v1/hosting/apps/app-1/domains")
            .query_param("pageToken", "t2");
        then.status(200)
            .json_body(json!({ "items": [{ "domainId": "d3" }], "links": [] }));
    });

    let listed = client(&server.base_url())
        .list_domains("app-1", Some(3))
        .await
        .expect("list domains");

    second.assert_calls(1);
    assert_eq!(domain_ids(&listed), ["d1", "d2", "d3"]);
    assert!(
        !listed.is_partial(),
        "that was the last page, so it is the whole list"
    );
    assert!(!listed.stopped_early);
    assert!(!listed.incomplete);
}

#[tokio::test]
async fn list_domains_does_not_report_an_early_stop_when_nothing_more_exists() {
    let server = MockServer::start_async().await;
    domains_first_page(
        &server,
        json!({ "items": [{ "domainId": "d1" }, { "domainId": "d2" }], "links": [] }),
    );

    let listed = client(&server.base_url())
        .list_domains("app-1", Some(1))
        .await
        .expect("list domains");

    // More held than wanted, but there was no further page to skip.
    assert_eq!(domain_ids(&listed), ["d1", "d2"]);
    assert!(!listed.stopped_early);
    assert!(!listed.incomplete);
}

#[tokio::test]
async fn list_domains_stops_at_a_repeated_next_link_and_reports_it() {
    let server = MockServer::start_async().await;
    domains_first_page(
        &server,
        json!({
            "items": [{ "domainId": "d1" }],
            "links": [{ "rel": "next", "href": "/v1/hosting/apps/app-1/domains?pageToken=loop" }]
        }),
    );
    let looping = server.mock(|when, then| {
        when.method(GET)
            .path("/v1/hosting/apps/app-1/domains")
            .query_param("pageToken", "loop");
        then.status(200).json_body(json!({
            "items": [{ "domainId": "d2" }],
            "links": [{ "rel": "next", "href": "/v1/hosting/apps/app-1/domains?pageToken=loop" }]
        }));
    });

    let listed = client(&server.base_url())
        .list_domains("app-1", None)
        .await
        .expect("a loop must not fail the listing");

    looping.assert_calls(1);
    assert_eq!(domain_ids(&listed), ["d1", "d2"]);
    assert!(listed.incomplete, "pages were left unread");
    assert!(listed.is_partial());
}

#[tokio::test]
async fn list_domains_reports_a_next_link_it_cannot_follow() {
    let server = MockServer::start_async().await;
    domains_first_page(
        &server,
        json!({
            "items": [{ "domainId": "d1" }],
            // No query to replay, so following it could never advance.
            "links": [{ "rel": "next", "href": "/v1/hosting/apps/app-1/domains" }]
        }),
    );

    let listed = client(&server.base_url())
        .list_domains("app-1", None)
        .await
        .expect("list domains");

    assert_eq!(domain_ids(&listed), ["d1"]);
    assert!(listed.incomplete);
    assert!(listed.is_partial(), "an unreadable page means no total");
}

#[test]
fn followed_pages_keep_their_body_out_of_the_transport_trace() {
    let body = br#"{"items":[{"hostname":"customer.example"}]}"#;
    assert_eq!(HostingClient::body_for_trace(true, body), body.as_slice());
    let traced = HostingClient::body_for_trace(false, body);
    assert!(!String::from_utf8_lossy(traced).contains("customer.example"));
    assert!(String::from_utf8_lossy(traced).contains("withheld"));
}

fn subscriptions_first_page(server: &MockServer, body: Value) -> httpmock::Mock<'_> {
    server.mock(|when, then| {
        when.method(GET)
            .path("/v1/hosting/subscriptions")
            .query_param("hostingProduct", "WEB_HOSTING")
            .query_param_missing("pageToken");
        then.status(200).json_body(body);
    })
}

fn subscription_ids(listed: &CombinedPages) -> Vec<&str> {
    listed
        .items
        .iter()
        .map(|item| item["subscriptionId"].as_str().unwrap_or(""))
        .collect()
}

#[tokio::test]
async fn list_subscriptions_combines_pages_by_following_next_links() {
    let server = MockServer::start_async().await;
    let first = subscriptions_first_page(
        &server,
        json!({
            "items": [{ "subscriptionId": "s1" }],
            "links": [{
                "rel": "next",
                "href": "/v1/hosting/subscriptions?hostingProduct=WEB_HOSTING&pageToken=t2"
            }]
        }),
    );
    let second = server.mock(|when, then| {
        when.method(GET)
            .path("/v1/hosting/subscriptions")
            .query_param("hostingProduct", "WEB_HOSTING")
            .query_param("pageToken", "t2")
            .header("authorization", "Bearer test-token");
        // `noise` is not in the spec; it must not survive on later pages only.
        then.status(200).json_body(json!({
            "items": [{ "subscriptionId": "s2", "noise": 1 }, { "subscriptionId": "s3" }],
            "links": [{ "rel": "self", "href": "/v1/hosting/subscriptions" }]
        }));
    });

    let listed = client(&server.base_url())
        .list_subscriptions("WEB_HOSTING")
        .await
        .expect("list subscriptions");

    first.assert_calls(1);
    second.assert_calls(1);
    assert_eq!(subscription_ids(&listed), ["s1", "s2", "s3"]);
    assert!(!listed.incomplete);
    assert!(
        listed.items.iter().all(|i| i.get("noise").is_none()),
        "{:?}",
        listed.items
    );
}

#[tokio::test]
async fn list_subscriptions_keeps_the_required_filter_when_a_next_link_drops_it() {
    let server = MockServer::start_async().await;
    subscriptions_first_page(
        &server,
        json!({
            "items": [{ "subscriptionId": "s1" }],
            "links": [{ "rel": "next", "href": "/v1/hosting/subscriptions?pageToken=t2" }]
        }),
    );
    let second = server.mock(|when, then| {
        when.method(GET)
            .path("/v1/hosting/subscriptions")
            .query_param("pageToken", "t2")
            .query_param("hostingProduct", "WEB_HOSTING");
        then.status(200)
            .json_body(json!({ "items": [{ "subscriptionId": "s2" }], "links": [] }));
    });

    let listed = client(&server.base_url())
        .list_subscriptions("WEB_HOSTING")
        .await
        .expect("list subscriptions");

    second.assert_calls(1);
    assert_eq!(subscription_ids(&listed), ["s1", "s2"]);
}

#[tokio::test]
async fn list_subscriptions_stops_at_a_repeated_next_link_and_reports_it() {
    let server = MockServer::start_async().await;
    subscriptions_first_page(
        &server,
        json!({
            "items": [{ "subscriptionId": "s1" }],
            "links": [{
                "rel": "next",
                "href": "/v1/hosting/subscriptions?hostingProduct=WEB_HOSTING&pageToken=loop"
            }]
        }),
    );
    let looping = server.mock(|when, then| {
        when.method(GET)
            .path("/v1/hosting/subscriptions")
            .query_param("pageToken", "loop");
        then.status(200).json_body(json!({
            "items": [{ "subscriptionId": "s2" }],
            "links": [{
                "rel": "next",
                "href": "/v1/hosting/subscriptions?hostingProduct=WEB_HOSTING&pageToken=loop"
            }]
        }));
    });

    let listed = client(&server.base_url())
        .list_subscriptions("WEB_HOSTING")
        .await
        .expect("a loop must not fail the listing");

    looping.assert_calls(1);
    assert_eq!(subscription_ids(&listed), ["s1", "s2"]);
    assert!(listed.incomplete, "pages were left unread");
}

#[tokio::test]
async fn list_subscriptions_reports_a_next_link_it_cannot_follow() {
    let server = MockServer::start_async().await;
    subscriptions_first_page(
        &server,
        json!({
            "items": [{ "subscriptionId": "s1" }],
            // No query to replay, so following it could never advance.
            "links": [{ "rel": "next", "href": "/v1/hosting/subscriptions" }]
        }),
    );

    let listed = client(&server.base_url())
        .list_subscriptions("WEB_HOSTING")
        .await
        .expect("list subscriptions");

    assert_eq!(subscription_ids(&listed), ["s1"]);
    assert!(listed.incomplete);
}
