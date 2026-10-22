use httpmock::prelude::*;
use serde_json::json;

use super::*;

fn client(base_url: &str) -> HostingClient {
    HostingClient::new(base_url, "test-token")
}

#[tokio::test]
async fn list_apps_sends_app_type_query_param() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/v1/hosting/apps")
                .query_param("appType", "NODEJS")
                .header("authorization", "Bearer test-token");
            then.status(200)
                .json_body(json!({ "items": [], "links": [] }));
        })
        .await;

    let body = client(&server.base_url())
        .list_apps("NODEJS", None, None)
        .await
        .expect("list apps");

    mock.assert_async().await;
    assert_eq!(body["items"], json!([]));
}

#[tokio::test]
async fn list_apps_sends_page_token_and_limit() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/v1/hosting/apps")
                .query_param("appType", "NODEJS")
                .query_param("pageToken", "tok-1")
                .query_param("pageSize", "5");
            then.status(200)
                .json_body(json!({ "items": [], "links": [] }));
        })
        .await;

    client(&server.base_url())
        .list_apps("NODEJS", Some("tok-1"), Some(5))
        .await
        .expect("list apps with pagination");

    mock.assert_async().await;
}

#[tokio::test]
async fn get_app_sends_bearer_auth() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/v1/hosting/apps/app-1")
                .header("authorization", "Bearer test-token");
            then.status(200).json_body(json!({ "id": "app-1" }));
        })
        .await;

    let body = client(&server.base_url())
        .get_app("app-1")
        .await
        .expect("get app");

    mock.assert_async().await;
    assert_eq!(body["id"], "app-1");
}

#[tokio::test]
async fn create_app_sends_app_type_query_and_json_body() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/v1/hosting/apps")
                .query_param("appType", "NODEJS")
                .json_body(json!({ "name": "my-app" }));
            then.status(202)
                .json_body(json!({ "operationId": "op-1", "status": "PENDING" }));
        })
        .await;

    let body = client(&server.base_url())
        .create_app("NODEJS", json!({ "name": "my-app" }))
        .await
        .expect("create app");

    mock.assert_async().await;
    assert_eq!(body["operationId"], "op-1");
}

#[tokio::test]
async fn update_app_sends_json_patch_content_type() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(PATCH)
                .path("/v1/hosting/apps/app-1")
                .header("content-type", "application/json-patch+json");
            then.status(200).json_body(json!({ "id": "app-1" }));
        })
        .await;

    let patch = json!([{ "op": "replace", "path": "/name", "value": "new-name" }]);
    let body = client(&server.base_url())
        .update_app("app-1", patch)
        .await
        .expect("update app");

    mock.assert_async().await;
    assert_eq!(body["id"], "app-1");
}

#[tokio::test]
async fn delete_app_returns_null_on_204() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(DELETE).path("/v1/hosting/apps/app-1");
            then.status(204);
        })
        .await;

    let body = client(&server.base_url())
        .delete_app("app-1")
        .await
        .expect("delete app");

    mock.assert_async().await;
    assert_eq!(body, json!(null));
}

#[tokio::test]
async fn get_app_status_hits_correct_path() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(GET).path("/v1/hosting/apps/app-1/status");
            then.status(200).json_body(json!({
                "status": "ACTIVE",
                "variants": [
                    { "variant": "PREVIEW", "status": "ACTIVE" },
                    { "variant": "PUBLISH", "status": "IDLE" }
                ]
            }));
        })
        .await;

    let body = client(&server.base_url())
        .get_app_status("app-1")
        .await
        .expect("get app status");

    mock.assert_async().await;
    assert!(body.get("variants").is_some());
}

#[tokio::test]
async fn restart_app_sends_variant_body() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/v1/hosting/apps/app-1/restarts")
                .json_body(json!({ "variant": "PREVIEW" }));
            then.status(200).json_body(json!({}));
        })
        .await;

    client(&server.base_url())
        .restart_app("app-1", "PREVIEW")
        .await
        .expect("restart app");

    mock.assert_async().await;
}

#[tokio::test]
async fn list_deployments_hits_correct_path() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/v1/hosting/apps/app-1/deployments")
                .header("authorization", "Bearer test-token");
            then.status(200)
                .json_body(json!({ "items": [], "links": [] }));
        })
        .await;

    client(&server.base_url())
        .list_deployments("app-1", None, None)
        .await
        .expect("list deployments");

    mock.assert_async().await;
}

#[tokio::test]
async fn get_deployment_hits_correct_path() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/v1/hosting/apps/app-1/deployments/dep-1");
            then.status(200)
                .json_body(json!({ "deploymentId": "dep-1" }));
        })
        .await;

    let body = client(&server.base_url())
        .get_deployment("app-1", "dep-1")
        .await
        .expect("get deployment");

    mock.assert_async().await;
    assert_eq!(body["deploymentId"], "dep-1");
}

#[tokio::test]
async fn create_deployment_posts_empty_json_body() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/v1/hosting/apps/app-1/deployments")
                .json_body(json!({}));
            then.status(202)
                .json_body(json!({ "deploymentId": "dep-1", "status": "PENDING" }));
        })
        .await;

    client(&server.base_url())
        .create_deployment("app-1")
        .await
        .expect("create deployment");

    mock.assert_async().await;
}

#[tokio::test]
async fn get_operation_hits_correct_path() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(GET).path("/v1/hosting/app-operations/op-1");
            then.status(200)
                .json_body(json!({ "operationId": "op-1", "status": "COMPLETED" }));
        })
        .await;

    let body = client(&server.base_url())
        .get_operation("op-1")
        .await
        .expect("get operation");

    mock.assert_async().await;
    assert_eq!(body["operationId"], "op-1");
}

#[tokio::test]
async fn create_import_sends_repo_and_branch() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/v1/hosting/apps/app-1/imports")
                .json_body(json!({ "repositoryFullName": "acme/my-app", "branch": "main" }));
            then.status(202).json_body(json!({ "id": "imp-1" }));
        })
        .await;

    client(&server.base_url())
        .create_import("app-1", "acme/my-app", "main")
        .await
        .expect("create import");

    mock.assert_async().await;
}

#[tokio::test]
async fn get_import_hits_correct_path() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/v1/hosting/apps/app-1/imports/imp-1");
            then.status(200).json_body(json!({ "id": "imp-1" }));
        })
        .await;

    client(&server.base_url())
        .get_import("app-1", "imp-1")
        .await
        .expect("get import");

    mock.assert_async().await;
}

/// A zip import's response arrives only after the whole upload, and a read
/// timeout runs from the start of the request, so the API client's read
/// timeout would cut off a large archive on a slow link.
#[tokio::test]
async fn create_import_zip_is_not_cut_off_by_the_api_read_timeout() {
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(POST).path("/v1/hosting/apps/app-1/imports");
            then.status(202)
                .json_body(json!({ "id": "imp-zip-1" }))
                .delay(std::time::Duration::from_millis(500));
        })
        .await;
    let hosting = HostingClient::with_clients(
        server.base_url(),
        "test-token",
        crate::http::make_api_client_with_read_timeout(std::time::Duration::from_millis(100)),
        make_api_upload_http_client(),
    );
    let tmp = tempfile::NamedTempFile::new().expect("tmp file");

    // Control: the API client really is that tight against this server.
    assert!(
        hosting
            .http
            .post(format!(
                "{}/v1/hosting/apps/app-1/imports",
                server.base_url()
            ))
            .body("x")
            .send()
            .await
            .is_err(),
        "the API client should time out against the slow server"
    );
    let result = hosting
        .create_import_zip("app-1", tmp.path())
        .await
        .expect("the upload client has no read timeout");

    assert_eq!(result["id"], "imp-zip-1");
}

#[tokio::test]
async fn create_import_zip_sends_multipart_to_imports_path() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/v1/hosting/apps/app-1/imports")
                .header_exists("content-type");
            then.status(202).json_body(json!({ "id": "imp-zip-1" }));
        })
        .await;

    let tmp = tempfile::NamedTempFile::new().expect("tmp file");
    let result = client(&server.base_url())
        .create_import_zip("app-1", tmp.path())
        .await
        .expect("create import zip");

    mock.assert_async().await;
    assert_eq!(result["id"], "imp-zip-1");
}

#[tokio::test]
async fn list_secrets_sends_variant_query_param() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/v1/hosting/apps/app-1/secrets")
                .query_param("variant", "PUBLISH");
            then.status(200).json_body(json!({ "items": [] }));
        })
        .await;

    client(&server.base_url())
        .list_secrets("app-1", Some("PUBLISH"))
        .await
        .expect("list secrets");

    mock.assert_async().await;
}

#[tokio::test]
async fn patch_secrets_sends_json_patch() {
    let server = MockServer::start_async().await;
    let patch = json!([{ "op": "add", "path": "/MY_SECRET", "value": "val" }]);
    let mock = server
        .mock_async(|when, then| {
            when.method(PATCH)
                .path("/v1/hosting/apps/app-1/secrets")
                .query_param("variant", "PREVIEW")
                .header("content-type", "application/json-patch+json")
                .json_body(patch.clone());
            then.status(200).json_body(json!({ "items": [] }));
        })
        .await;

    client(&server.base_url())
        .patch_secrets("app-1", "PREVIEW", patch)
        .await
        .expect("patch secrets");

    mock.assert_async().await;
}

#[tokio::test]
async fn list_logs_sends_target_query_param() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/v1/hosting/apps/app-1/logs")
                .query_param("target", "PREVIEW");
            then.status(200)
                .json_body(json!({ "items": [], "links": [] }));
        })
        .await;

    client(&server.base_url())
        .list_logs("app-1", Some("PREVIEW"), None, None, None, None, None)
        .await
        .expect("list logs");

    mock.assert_async().await;
}

#[tokio::test]
async fn get_runtime_hits_correct_path() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(GET).path("/v1/hosting/apps/app-1/runtime");
            then.status(200).json_body(json!([{
                "runtime": "NODEJS",
                "version": "22.11.0"
            }]));
        })
        .await;

    client(&server.base_url())
        .get_runtime("app-1")
        .await
        .expect("get runtime");

    mock.assert_async().await;
}

#[tokio::test]
async fn list_domains_hits_correct_path() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(GET).path("/v1/hosting/apps/app-1/domains");
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
        !listed.stopped_early,
        "that was the last page, so it is the whole list"
    );
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
}

#[tokio::test]
async fn attach_domain_sends_hostname() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/v1/hosting/apps/app-1/domains")
                .json_body(json!({ "hostname": "www.example.com" }));
            then.status(200).json_body(json!({ "id": "dom-1" }));
        })
        .await;

    client(&server.base_url())
        .attach_domain("app-1", "www.example.com")
        .await
        .expect("attach domain");

    mock.assert_async().await;
}

#[tokio::test]
async fn detach_domain_returns_null_on_204() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(DELETE)
                .path("/v1/hosting/apps/app-1/domains/dom-1");
            then.status(204);
        })
        .await;

    let body = client(&server.base_url())
        .detach_domain("app-1", "dom-1")
        .await
        .expect("detach domain");

    mock.assert_async().await;
    assert_eq!(body, json!(null));
}

#[tokio::test]
async fn list_subscriptions_hits_correct_path() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/v1/hosting/subscriptions")
                .query_param("hostingProduct", "WEB_HOSTING");
            then.status(200)
                .json_body(json!({ "items": [], "links": [] }));
        })
        .await;

    client(&server.base_url())
        .list_subscriptions("WEB_HOSTING")
        .await
        .expect("list subscriptions");

    mock.assert_async().await;
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

#[tokio::test]
async fn attach_subscription_sends_subscription_id() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(PUT)
                .path("/v1/hosting/apps/app-1/subscription")
                .json_body(json!({ "subscriptionId": "sub-1" }));
            then.status(200)
                .json_body(json!({ "subscriptionId": "sub-1" }));
        })
        .await;

    client(&server.base_url())
        .attach_subscription("app-1", "sub-1")
        .await
        .expect("attach subscription");

    mock.assert_async().await;
}

#[tokio::test]
async fn http_error_is_returned_as_client_error() {
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(GET).path("/v1/hosting/apps/not-found");
            then.status(404)
                .json_body(json!({ "code": "NOT_FOUND", "message": "app not found" }));
        })
        .await;

    let err = client(&server.base_url())
        .get_app("not-found")
        .await
        .expect_err("expected 404 error");

    assert!(matches!(err, ClientError::Http { status: 404, .. }));
}

#[tokio::test]
async fn get_agent_token_posts_empty_body_and_returns_url_and_token() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/v1/hosting/apps/NODEJS-app-1/agent-token")
                .header("authorization", "Bearer test-token")
                .json_body(json!({}));
            then.status(200).json_body(json!({
                "agentUrl": "https://app-1.agent.example",
                "token": "minted-agent-jwt"
            }));
        })
        .await;

    let body = client(&server.base_url())
        .get_agent_token("app-1", "NODEJS")
        .await
        .expect("get agent token");

    mock.assert_async().await;
    assert_eq!(body["agentUrl"], "https://app-1.agent.example");
    assert_eq!(body["token"], "minted-agent-jwt");
}

#[tokio::test]
async fn ensure_database_tunnel_session_posts_to_prefixed_app_path() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/v1/hosting/apps/MHWP-app-1/database-tunnel")
                .header("authorization", "Bearer test-token")
                .json_body(json!({}));
            then.status(200).json_body(json!({
                "sessionId": "s1",
                "url": "https://dbt-s1.c1.pma.example",
                "pollUrl": "https://dbt-s1.c1.pma.example/healthz",
                "token": "relay-token"
            }));
        })
        .await;

    let body = client(&server.base_url())
        .ensure_database_tunnel_session("app-1", "MHWP")
        .await
        .expect("ensure database tunnel session");

    mock.assert_async().await;
    assert_eq!(body["url"], "https://dbt-s1.c1.pma.example");
    assert_eq!(body["pollUrl"], "https://dbt-s1.c1.pma.example/healthz");
    assert_eq!(body["token"], "relay-token");
}
