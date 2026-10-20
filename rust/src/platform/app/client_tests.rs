use httpmock::prelude::*;

use super::*;

#[tokio::test]
async fn list_applications_sends_all_app_registry_statuses() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/v1/apps/app-registry-subgraph")
                .header("authorization", "Bearer test-token")
                .is_true(|req| {
                    let body = req.body_string();
                    body.contains("ApplicationsList")
                        && body.contains(
                            r#""in":["ACTIVE","ARCHIVED","BLOCKED","INACTIVE","VERIFYING"]"#,
                        )
                });
            then.status(200).json_body(json!({
                "data": {
                    "applications": {
                        "edges": [
                            { "node": { "id": "a1", "name": "active-app", "status": "ACTIVE" } },
                            { "node": { "id": "a2", "name": "inactive-app", "status": "INACTIVE" } }
                        ]
                    }
                }
            }));
        })
        .await;

    let data = ApplicationClient::new(server.base_url(), "test-token")
        .list_applications()
        .await
        .expect("list applications");

    mock.assert_async().await;
    assert_eq!(data.as_array().expect("array").len(), 2);
    assert_eq!(data[1]["status"], "INACTIVE");
}

#[tokio::test]
async fn activate_release_posts_mutation_with_ids() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/v1/apps/app-registry-subgraph")
                .header("authorization", "Bearer test-token")
                .is_true(|req| {
                    let body = req.body_string();
                    body.contains("activateRelease")
                        && body.contains("app-123")
                        && body.contains("rel-456")
                });
            then.status(200).json_body(json!({
                "data": { "activateRelease": { "id": "rel-456", "status": "ACTIVE" } }
            }));
        })
        .await;

    let data = ApplicationClient::new(server.base_url(), "test-token")
        .activate_release("app-123", "rel-456")
        .await
        .expect("activate release");

    mock.assert_async().await;
    assert_eq!(data["activateRelease"]["status"], "ACTIVE");
}

#[tokio::test]
async fn list_enabled_store_applications_sends_store_id_variable() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/v1/apps/app-registry-subgraph")
                .header("authorization", "Bearer test-token")
                .is_true(|req| {
                    let body = req.body_string();
                    body.contains("EnabledStoreApplications")
                        && body.contains("enabledStoreApplications")
                        && body.contains(r#""storeId":"store-abc""#)
                        && body.contains("label")
                });
            then.status(200).json_body(json!({
                "data": {
                    "enabledStoreApplications": [{
                        "id": "app-1",
                        "name": "my-app",
                        "label": "My App",
                        "status": "ACTIVE",
                        "release": { "id": "rel-1", "version": "1.0.0" }
                    }]
                }
            }));
        })
        .await;

    let data = ApplicationClient::new(server.base_url(), "test-token")
        .list_enabled_store_applications("store-abc")
        .await
        .expect("list enabled store applications");

    mock.assert_async().await;
    assert_eq!(data.as_array().expect("array").len(), 1);
    assert_eq!(data[0]["name"], "my-app");
    assert_eq!(data[0]["label"], "My App");
    assert_eq!(data[0]["release"]["version"], "1.0.0");
}

#[tokio::test]
async fn list_enabled_store_applications_empty_list_is_success() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/v1/apps/app-registry-subgraph")
                .header("authorization", "Bearer test-token")
                .is_true(|req| req.body_string().contains("EnabledStoreApplications"));
            then.status(200).json_body(json!({
                "data": { "enabledStoreApplications": [] }
            }));
        })
        .await;

    let data = ApplicationClient::new(server.base_url(), "test-token")
        .list_enabled_store_applications("store-empty")
        .await
        .expect("empty enablements");

    mock.assert_async().await;
    assert_eq!(data, json!([]));
}

#[tokio::test]
async fn create_release_sends_settings_input_and_returns_settings() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/v1/apps/app-registry-subgraph")
                .header("authorization", "Bearer test-token")
                .is_true(|req| {
                    let body = req.body_string();
                    body.contains("CreateRelease")
                        && body.contains("settings { id groupSlug appSettingSlug entryPath capabilities order title }")
                        && body.contains(r#""entryPath":"/settings/godaddy-tax""#)
                });
            then.status(200).json_body(json!({
                "data": {
                    "createRelease": {
                        "id": "rel-1",
                        "version": "1.0.0",
                        "settings": [{
                            "id": "setting-1",
                            "groupSlug": "tax-center",
                            "appSettingSlug": "godaddy-tax",
                            "entryPath": "/settings/godaddy-tax",
                            "capabilities": ["read", "write"],
                            "order": 10,
                            "title": "GoDaddy Tax"
                        }]
                    }
                }
            }));
        })
        .await;

    let input = json!({
        "applicationId": "app-123",
        "version": "1.0.0",
        "settings": [{
            "groupSlug": "tax-center",
            "appSettingSlug": "godaddy-tax",
            "entryPath": "/settings/godaddy-tax",
            "presentation": { "type": "form", "schemaVersion": "settings-form-v1", "sections": [] }
        }]
    });
    let data = ApplicationClient::new(server.base_url(), "test-token")
        .create_release(input)
        .await
        .expect("create release");

    mock.assert_async().await;
    assert_eq!(
        data["createRelease"]["settings"][0]["entryPath"],
        "/settings/godaddy-tax"
    );
}

#[tokio::test]
async fn create_release_sends_native_extensions_input_and_selects_them() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/v1/apps/app-registry-subgraph")
                .header("authorization", "Bearer test-token")
                .is_true(|req| {
                    let body = req.body_string();
                    body.contains("CreateRelease")
                        && body
                            .contains("nativeExtensions { id name packageName contact platform }")
                        && body.contains(r#""platform":"ANDROID""#)
                        && body.contains(r#""packageName":"com.example.app""#)
                });
            then.status(200).json_body(json!({
                "data": {
                    "createRelease": {
                        "id": "rel-1",
                        "version": "1.0.11",
                        "nativeExtensions": [{
                            "id": "ne-1",
                            "name": "My Display Name",
                            "packageName": "com.example.app",
                            "contact": "support@example.com",
                            "platform": "ANDROID"
                        }]
                    }
                }
            }));
        })
        .await;

    let input = json!({
        "applicationId": "app-123",
        "version": "1.0.11",
        "nativeExtensions": [{
            "platform": "ANDROID",
            "name": "My Display Name",
            "contact": "support@example.com",
            "packageName": "com.example.app"
        }]
    });
    let data = ApplicationClient::new(server.base_url(), "test-token")
        .create_release(input)
        .await
        .expect("create release");

    mock.assert_async().await;
    assert_eq!(data["createRelease"]["nativeExtensions"][0]["id"], "ne-1");
    assert_eq!(
        data["createRelease"]["nativeExtensions"][0]["packageName"],
        "com.example.app"
    );
}

#[tokio::test]
async fn activate_release_surfaces_graphql_errors() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(POST).path("/v1/apps/app-registry-subgraph");
            // GraphQL reports failures as HTTP 200 with an `errors` array.
            then.status(200).json_body(json!({
                "errors": [{ "message": "release not found" }]
            }));
        })
        .await;

    let err = ApplicationClient::new(server.base_url(), "test-token")
        .activate_release("app-123", "missing")
        .await
        .expect_err("graphql errors should surface");

    mock.assert_async().await;
    assert!(
        matches!(err, ClientError::GraphQL(msg) if msg.contains("release not found")),
        "expected GraphQL error variant"
    );
}

/// Distinct from `activate_release_surfaces_graphql_errors`: GraphQL
/// reports failures as HTTP 200 with an `errors` array, but the
/// transport itself (auth rejected, gateway down, etc.) can also fail
/// at the HTTP layer with a non-2xx status. That path maps to
/// `ClientError::Http`, not `ClientError::GraphQL`.
#[tokio::test]
async fn query_maps_a_non_2xx_status_to_a_http_client_error() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(POST).path("/v1/apps/app-registry-subgraph");
            then.status(401).body("unauthorized");
        })
        .await;

    let err = ApplicationClient::new(server.base_url(), "test-token")
        .get_application("test-app")
        .await
        .expect_err("non-2xx status should surface as an HTTP error");

    mock.assert_async().await;
    assert!(
        matches!(err, ClientError::Http { status: 401, ref body } if body.contains("unauthorized")),
        "expected Http error variant, got: {err:?}"
    );
}

#[tokio::test]
async fn get_application_with_releases_selects_client_id_and_subscriptions() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/v1/apps/app-registry-subgraph")
                .is_true(|req| {
                    let body = req.body_string();
                    body.contains("ApplicationWithLatestRelease")
                        && body.contains("clientId")
                        && body.contains("subscriptions { name url events }")
                });
            then.status(200).json_body(json!({
                "data": {
                    "application": {
                        "id": "app-1",
                        "clientId": "client-1",
                        "releases": {
                            "edges": [{
                                "node": {
                                    "id": "rel-1",
                                    "subscriptions": [{
                                        "name": "order-notifications",
                                        "url": "https://proxy.example.com/webhooks/orders",
                                        "events": ["commerce.order.created"]
                                    }]
                                }
                            }]
                        }
                    }
                }
            }));
        })
        .await;

    let data = ApplicationClient::new(server.base_url(), "test-token")
        .get_application_with_releases("test-app")
        .await
        .expect("get application with releases");

    mock.assert_async().await;
    assert_eq!(data["application"]["clientId"], "client-1");
    assert_eq!(
        data["application"]["releases"]["edges"][0]["node"]["subscriptions"][0]["name"],
        "order-notifications"
    );
}

#[tokio::test]
async fn update_application_sends_non_lifecycle_fields() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/v1/apps/app-registry-subgraph")
                .is_true(|req| {
                    let body = req.body_string();
                    body.contains("updateApplication") && body.contains(r#""label":"Updated app""#)
                });
            then.status(200).json_body(json!({
                "data": { "updateApplication": { "id": "app-1", "label": "Updated app" } }
            }));
        })
        .await;

    let data = ApplicationClient::new(server.base_url(), "test-token")
        .update_application("app-1", json!({ "label": "Updated app" }))
        .await
        .expect("update application");

    mock.assert_async().await;
    assert_eq!(data["updateApplication"]["label"], "Updated app");
}

// httpmock can't sequence responses, so retries are verified by hit count
// (exhaustion) rather than a fail-then-succeed sequence.

#[tokio::test]
async fn upload_rejects_oversize_file_without_uploading() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(PUT).path("/upload");
            then.status(200);
        })
        .await;

    let err = ApplicationClient::new(server.base_url(), "test-token")
        .upload_artifact(
            &server.url("/upload"),
            "up-1",
            &json!({}),
            Some(4), // max 4 bytes
            Bytes::from_static(b"way too big"),
            UploadOptions {
                max_attempts: 3,
                base_delay_ms: 0,
            },
        )
        .await
        .expect_err("oversize should fail before uploading");

    assert!(
        matches!(err, ClientError::TooLarge { .. }),
        "unexpected: {err}"
    );
    assert_eq!(mock.calls_async().await, 0);
}

#[tokio::test]
async fn upload_does_not_retry_on_4xx() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(PUT).path("/upload");
            then.status(403).body("denied");
        })
        .await;

    let err = ApplicationClient::new(server.base_url(), "test-token")
        .upload_artifact(
            &server.url("/upload"),
            "up-1",
            &json!({}),
            None,
            Bytes::from_static(b"data"),
            UploadOptions {
                max_attempts: 3,
                base_delay_ms: 0,
            },
        )
        .await
        .expect_err("4xx should fail immediately");

    assert!(
        matches!(err, ClientError::Http { status: 403, .. }),
        "unexpected: {err}"
    );
    assert_eq!(mock.calls_async().await, 1);
}

#[tokio::test]
async fn upload_retries_on_5xx_until_exhausted() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(PUT).path("/upload");
            then.status(503).body("try later");
        })
        .await;

    let err = ApplicationClient::new(server.base_url(), "test-token")
        .upload_artifact(
            &server.url("/upload"),
            "up-1",
            &json!({}),
            None,
            Bytes::from_static(b"data"),
            UploadOptions {
                max_attempts: 3,
                base_delay_ms: 0,
            },
        )
        .await
        .expect_err("exhausted retries should fail");

    assert!(
        matches!(err, ClientError::Http { status: 503, .. }),
        "unexpected: {err}"
    );
    assert_eq!(mock.calls_async().await, 3);
}

#[tokio::test]
async fn upload_strips_meta_upload_id_and_returns_metadata() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(PUT)
                .path("/upload")
                .header("x-amz-signature", "sig")
                // assert x-amz-meta-upload-id was stripped
                .is_true(|req| {
                    !req.headers_vec()
                        .iter()
                        .any(|(k, _)| k.eq_ignore_ascii_case("x-amz-meta-upload-id"))
                });
            then.status(200).header("etag", "\"abc123\"");
        })
        .await;

    let result = ApplicationClient::new(server.base_url(), "test-token")
        .upload_artifact(
            &server.url("/upload"),
            "up-42",
            &json!({
                "x-amz-signature": "sig",
                "x-amz-meta-upload-id": "should-be-stripped",
            }),
            None,
            Bytes::from_static(b"hello"),
            UploadOptions {
                max_attempts: 3,
                base_delay_ms: 0,
            },
        )
        .await
        .expect("upload should succeed");

    mock.assert_async().await;
    assert_eq!(result.upload_id, "up-42");
    assert_eq!(result.status, 200);
    assert_eq!(result.size_bytes, 5);
    assert_eq!(result.etag.as_deref(), Some("\"abc123\""));
}

#[tokio::test]
async fn upload_rejects_invalid_header_without_uploading() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(PUT).path("/upload");
            then.status(200);
        })
        .await;

    let err = ApplicationClient::new(server.base_url(), "test-token")
        .upload_artifact(
            &server.url("/upload"),
            "up-1",
            &json!({ "bad header name": "x" }), // space in name → invalid
            None,
            Bytes::from_static(b"data"),
            UploadOptions {
                max_attempts: 3,
                base_delay_ms: 0,
            },
        )
        .await
        .expect_err("invalid header should fail before uploading");

    assert!(
        matches!(err, ClientError::InvalidHeader(_)),
        "unexpected: {err}"
    );
    assert_eq!(mock.calls_async().await, 0);
}

/// S3 answers a presigned PUT only after the whole body arrives, and a read
/// timeout runs from the start of the request, so the API client's read
/// timeout would cut off a large bundle on a slow link.
#[tokio::test]
async fn upload_artifact_is_not_cut_off_by_the_api_read_timeout() {
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(PUT).path("/artifact");
            then.status(200)
                .delay(std::time::Duration::from_millis(500));
        })
        .await;
    let client = ApplicationClient::with_clients(
        server.base_url(),
        "test-token",
        crate::http::make_api_client_with_read_timeout(std::time::Duration::from_millis(100)),
        make_storage_upload_http_client(),
    );

    // Control: the API client really is that tight against this server.
    assert!(
        client
            .client
            .put(server.url("/artifact"))
            .body("x")
            .send()
            .await
            .is_err(),
        "the API client should time out against the slow server"
    );
    client
        .upload_artifact(
            &server.url("/artifact"),
            "upload-1",
            &json!({}),
            None,
            Bytes::from_static(b"bundle"),
            UploadOptions {
                max_attempts: 1,
                base_delay_ms: 0,
            },
        )
        .await
        .expect("the upload client has no read timeout");
}

#[tokio::test]
async fn upload_artifact_accepts_reusable_shared_payload() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(PUT)
                .path("/artifact")
                .body("shared extension bundle");
            then.status(200);
        })
        .await;
    let client = ApplicationClient::new(server.base_url(), "test-token");
    let payload = Bytes::from_static(b"shared extension bundle");
    let first_upload = payload.clone();
    let second_upload = payload.clone();
    assert_eq!(first_upload.as_ptr(), second_upload.as_ptr());

    for (upload_id, bytes) in [("upload-1", first_upload), ("upload-2", second_upload)] {
        client
            .upload_artifact(
                &server.url("/artifact"),
                upload_id,
                &json!({}),
                None,
                bytes,
                UploadOptions {
                    max_attempts: 1,
                    base_delay_ms: 0,
                },
            )
            .await
            .expect("upload shared payload");
    }

    mock.assert_calls_async(2).await;
}
