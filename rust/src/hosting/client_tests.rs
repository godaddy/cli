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
                .query_param("limit", "5");
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
            then.status(200)
                .json_body(json!({ "preview": "ACTIVE", "publish": "IDLE" }));
        })
        .await;

    let body = client(&server.base_url())
        .get_app_status("app-1")
        .await
        .expect("get app status");

    mock.assert_async().await;
    assert!(body.get("preview").is_some());
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
            then.status(200).json_body(json!({ "id": "dep-1" }));
        })
        .await;

    let body = client(&server.base_url())
        .get_deployment("app-1", "dep-1")
        .await
        .expect("get deployment");

    mock.assert_async().await;
    assert_eq!(body["id"], "dep-1");
}

#[tokio::test]
async fn create_deployment_posts_with_no_body() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(POST).path("/v1/hosting/apps/app-1/deployments");
            then.status(202)
                .json_body(json!({ "id": "dep-1", "status": "PENDING" }));
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
                .json_body(json!({ "id": "op-1", "status": "COMPLETED" }));
        })
        .await;

    let body = client(&server.base_url())
        .get_operation("op-1")
        .await
        .expect("get operation");

    mock.assert_async().await;
    assert_eq!(body["id"], "op-1");
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

#[tokio::test]
async fn get_github_connection_hits_correct_path() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/v1/hosting/settings/github/connection");
            then.status(200).json_body(json!({ "connected": true }));
        })
        .await;

    let body = client(&server.base_url())
        .get_github_connection()
        .await
        .expect("get github connection");

    mock.assert_async().await;
    assert_eq!(body["connected"], true);
}

#[tokio::test]
async fn list_github_repos_hits_correct_path() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/v1/hosting/settings/github/repositories");
            then.status(200)
                .json_body(json!({ "items": [], "links": [] }));
        })
        .await;

    client(&server.base_url())
        .list_github_repos(None, None)
        .await
        .expect("list github repos");

    mock.assert_async().await;
}

#[tokio::test]
async fn list_github_branches_hits_correct_path() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/v1/hosting/settings/github/repositories/acme/my-app/branches");
            then.status(200)
                .json_body(json!({ "items": [], "links": [] }));
        })
        .await;

    client(&server.base_url())
        .list_github_branches("acme", "my-app", None, None)
        .await
        .expect("list github branches");

    mock.assert_async().await;
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
async fn sync_secrets_sends_body() {
    let server = MockServer::start_async().await;
    let body = json!({
        "variant": "PREVIEW",
        "operations": { "additions": [{ "name": "MY_SECRET", "value": "val" }] }
    });
    let mock = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/v1/hosting/apps/app-1/secrets/sync")
                .json_body(body.clone());
            then.status(200).json_body(json!({ "items": [] }));
        })
        .await;

    client(&server.base_url())
        .sync_secrets("app-1", body)
        .await
        .expect("sync secrets");

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
            then.status(200).json_body(json!({ "status": "RUNNING" }));
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
        .list_domains("app-1", None, None)
        .await
        .expect("list domains");

    mock.assert_async().await;
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
            when.method(GET).path("/v1/hosting/subscriptions");
            then.status(200)
                .json_body(json!({ "items": [], "links": [] }));
        })
        .await;

    client(&server.base_url())
        .list_subscriptions(None, None)
        .await
        .expect("list subscriptions");

    mock.assert_async().await;
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
