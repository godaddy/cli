use httpmock::prelude::*;
use serde_json::json;

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
                        && body.contains(r#""in":["ACTIVE","ARCHIVED","BLOCKED","INACTIVE","VERIFYING"]"#)
                });
            then.status(200).json_body(json!({
                "data": {
                    "applications": {
                        "edges": [
                            { "node": { "id": "a1", "label": "Active", "name": "active-app", "description": null, "status": "ACTIVE", "url": null, "proxyUrl": null } },
                            { "node": { "id": "a2", "label": "Inactive", "name": "inactive-app", "description": null, "status": "INACTIVE", "url": null, "proxyUrl": null } }
                        ]
                    }
                }
            }));
        })
        .await;

    let nodes = ApplicationClient::new(server.base_url(), "test-token")
        .list_applications()
        .await
        .expect("list applications");

    mock.assert_async().await;
    assert_eq!(nodes.len(), 2);
    assert_eq!(
        nodes[1].status,
        applications_list::ApplicationStatus::INACTIVE
    );
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
                    body.contains("ActivateRelease")
                        && body.contains("app-123")
                        && body.contains("rel-456")
                });
            then.status(200).json_body(json!({
                "data": {
                    "activateRelease": {
                        "id": "rel-456",
                        "version": "1.0.0",
                        "description": null,
                        "status": "ACTIVE",
                        "activatedAt": "2026-01-01T00:00:00Z",
                        "createdAt": "2026-01-01T00:00:00Z",
                        "updatedAt": "2026-01-01T00:00:00Z"
                    }
                }
            }));
        })
        .await;

    let release = ApplicationClient::new(server.base_url(), "test-token")
        .activate_release("app-123", "rel-456")
        .await
        .expect("activate release")
        .expect("release present");

    mock.assert_async().await;
    assert_eq!(
        release.status,
        activate_release::ApplicationReleaseStatus::ACTIVE
    );
}

#[tokio::test]
async fn list_enabled_store_applications_sends_store_id_variable() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/v1/apps/app-registry-subgraph")
                .is_true(|req| {
                    let body = req.body_string();
                    body.contains("EnabledStoreApplications")
                        && body.contains(r#""storeId":"store-abc""#)
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

    let apps = ApplicationClient::new(server.base_url(), "test-token")
        .list_enabled_store_applications("store-abc")
        .await
        .expect("list enabled store applications");

    mock.assert_async().await;
    assert_eq!(apps.len(), 1);
    assert_eq!(apps[0].name, "my-app");
    assert_eq!(apps[0].label, "My App");
    assert_eq!(
        apps[0].release.as_ref().map(|r| r.version.as_str()),
        Some("1.0.0")
    );
}

#[tokio::test]
async fn list_enabled_store_applications_empty_list_is_success() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/v1/apps/app-registry-subgraph")
                .is_true(|req| req.body_string().contains("EnabledStoreApplications"));
            then.status(200).json_body(json!({
                "data": { "enabledStoreApplications": [] }
            }));
        })
        .await;

    let apps = ApplicationClient::new(server.base_url(), "test-token")
        .list_enabled_store_applications("store-empty")
        .await
        .expect("empty enablements");

    mock.assert_async().await;
    assert!(apps.is_empty());
}

#[tokio::test]
async fn create_release_sends_settings_input_and_returns_settings() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/v1/apps/app-registry-subgraph")
                .is_true(|req| {
                    let body = req.body_string();
                    body.contains("CreateRelease")
                        && body.contains(r#""entryPath":"/settings/godaddy-tax""#)
                        && body.contains(r#""groupSlug":"tax-center""#)
                });
            then.status(200).json_body(json!({
                "data": {
                    "createRelease": {
                        "id": "rel-1",
                        "version": "1.0.0",
                        "description": null,
                        "createdAt": "2026-01-01T00:00:00Z",
                        "uiExtensions": [],
                        "settings": [{
                            "id": "setting-1",
                            "groupSlug": "tax-center",
                            "appSettingSlug": "godaddy-tax",
                            "entryPath": "/settings/godaddy-tax",
                            "capabilities": ["read", "write"],
                            "order": 10,
                            "title": "GoDaddy Tax",
                            "titleKey": null,
                            "description": null,
                            "descriptionKey": null,
                            "iconName": null,
                            "iconLibrary": null,
                            "metadata": null,
                            "presentation": { "type": "form", "schemaVersion": "settings-form-v1", "sections": [] }
                        }]
                    }
                }
            }));
        })
        .await;

    let input = create_release::MutationCreateReleaseInput {
        application_id: "app-123".to_owned(),
        version: "1.0.0".to_owned(),
        description: None,
        actions: None,
        application_dependencies: None,
        feature_dependencies: None,
        mcp_servers: None,
        native_extensions: None,
        subscriptions: None,
        ui_extensions: None,
        settings: Some(vec![create_release::ApplicationSettingCreateInput {
            group_slug: "tax-center".to_owned(),
            app_setting_slug: "godaddy-tax".to_owned(),
            entry_path: "/settings/godaddy-tax".to_owned(),
            presentation: json!({ "type": "form", "schemaVersion": "settings-form-v1", "sections": [] }),
            capabilities: None,
            description: None,
            description_key: None,
            icon_name: None,
            icon_library: None,
            metadata: None,
            order: None,
            title: None,
            title_key: None,
        }]),
    };
    let release = ApplicationClient::new(server.base_url(), "test-token")
        .create_release(input)
        .await
        .expect("create release")
        .expect("release present");

    mock.assert_async().await;
    assert_eq!(release.settings[0].entry_path, "/settings/godaddy-tax");
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
                .is_true(|req| req.body_string().contains("ApplicationWithLatestRelease"));
            then.status(200).json_body(json!({
                "data": {
                    "application": {
                        "id": "app-1",
                        "label": "My App",
                        "name": "my-app",
                        "description": null,
                        "status": "ACTIVE",
                        "url": null,
                        "proxyUrl": null,
                        "authorizationScopes": [],
                        "clientId": "client-1",
                        "releases": {
                            "edges": [{
                                "node": {
                                    "id": "rel-1",
                                    "version": "1.0.0",
                                    "description": null,
                                    "createdAt": "2026-01-01T00:00:00Z",
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

    let app = ApplicationClient::new(server.base_url(), "test-token")
        .get_application_with_releases("test-app")
        .await
        .expect("get application with releases")
        .expect("application present");

    mock.assert_async().await;
    assert_eq!(app.client_id.as_deref(), Some("client-1"));
    let edges = app
        .releases
        .expect("releases connection")
        .edges
        .expect("edges");
    let node = edges[0]
        .as_ref()
        .expect("edge")
        .node
        .as_ref()
        .expect("node");
    assert_eq!(node.subscriptions[0].name, "order-notifications");
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
                    body.contains("UpdateApplication") && body.contains(r#""label":"Updated app""#)
                });
            then.status(200).json_body(json!({
                "data": {
                    "updateApplication": {
                        "id": "app-1",
                        "clientId": null,
                        "label": "Updated app",
                        "name": "my-app",
                        "description": null,
                        "status": "ACTIVE",
                        "url": null,
                        "proxyUrl": null,
                        "authorizationScopes": []
                    }
                }
            }));
        })
        .await;

    let input = update_application::MutationUpdateApplicationInput {
        label: Some("Updated app".to_owned()),
        description: None,
        authorization_scopes: None,
        distribution_type: None,
        name: None,
        proxy_url: None,
        redirect_uris: None,
        url: None,
    };
    let app = ApplicationClient::new(server.base_url(), "test-token")
        .update_application("app-1", input)
        .await
        .expect("update application")
        .expect("application present");

    mock.assert_async().await;
    assert_eq!(app.label, "Updated app");
}

#[tokio::test]
async fn get_application_returns_selected_fields() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/v1/apps/app-registry-subgraph")
                .is_true(|req| req.body_string().contains("Application"));
            then.status(200).json_body(json!({
                "data": {
                    "application": {
                        "id": "app-1",
                        "label": "My App",
                        "name": "my-app",
                        "description": "desc",
                        "status": "ACTIVE",
                        "url": "https://example.com",
                        "proxyUrl": "https://proxy.example.com"
                    }
                }
            }));
        })
        .await;

    let app = ApplicationClient::new(server.base_url(), "test-token")
        .get_application("my-app")
        .await
        .expect("get application")
        .expect("application present");

    mock.assert_async().await;
    assert_eq!(app.name, "my-app");
    assert_eq!(app.proxy_url.as_deref(), Some("https://proxy.example.com"));
}

#[tokio::test]
async fn enable_application_sends_application_name_and_store_id() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/v1/apps/app-registry-subgraph")
                .is_true(|req| {
                    let body = req.body_string();
                    body.contains("EnableApplication")
                        && body.contains(r#""applicationName":"my-app""#)
                        && body.contains(r#""storeId":"store-1""#)
                });
            then.status(200)
                .json_body(json!({ "data": { "enableStoreApplication": { "id": "app-1" } } }));
        })
        .await;

    let input = enable_application::MutationEnableStoreApplicationInput {
        application_name: "my-app".to_owned(),
        store_id: "store-1".to_owned(),
    };
    let app = ApplicationClient::new(server.base_url(), "test-token")
        .enable_application(input)
        .await
        .expect("enable application")
        .expect("application present");

    mock.assert_async().await;
    assert_eq!(app.id, "app-1");
}

#[tokio::test]
async fn disable_application_sends_application_name_and_store_id() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/v1/apps/app-registry-subgraph")
                .is_true(|req| {
                    let body = req.body_string();
                    body.contains("DisableApplication")
                        && body.contains(r#""applicationName":"my-app""#)
                        && body.contains(r#""storeId":"store-1""#)
                });
            then.status(200)
                .json_body(json!({ "data": { "disableStoreApplication": { "id": "app-1" } } }));
        })
        .await;

    let input = disable_application::MutationDisableStoreApplicationInput {
        application_name: "my-app".to_owned(),
        store_id: "store-1".to_owned(),
    };
    let app = ApplicationClient::new(server.base_url(), "test-token")
        .disable_application(input)
        .await
        .expect("disable application")
        .expect("application present");

    mock.assert_async().await;
    assert_eq!(app.id, "app-1");
}

#[tokio::test]
async fn archive_application_sends_id_and_returns_lifecycle_fields() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/v1/apps/app-registry-subgraph")
                .is_true(|req| {
                    let body = req.body_string();
                    body.contains("ArchiveApplication") && body.contains(r#""id":"app-1""#)
                });
            then.status(200).json_body(json!({
                "data": {
                    "archiveApplication": {
                        "id": "app-1",
                        "label": "My App",
                        "name": "my-app",
                        "status": "ARCHIVED",
                        "createdAt": "2026-01-01T00:00:00Z",
                        "archivedAt": "2026-02-01T00:00:00Z"
                    }
                }
            }));
        })
        .await;

    let app = ApplicationClient::new(server.base_url(), "test-token")
        .archive_application("app-1")
        .await
        .expect("archive application")
        .expect("application present");

    mock.assert_async().await;
    assert_eq!(app.status, archive_application::ApplicationStatus::ARCHIVED);
}

#[tokio::test]
async fn create_application_sends_input_and_returns_credentials() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/v1/apps/app-registry-subgraph")
                .is_true(|req| {
                    let body = req.body_string();
                    body.contains("CreateApplication") && body.contains(r#""name":"my-app""#)
                });
            then.status(200).json_body(json!({
                "data": {
                    "createApplication": {
                        "id": "app-1",
                        "clientId": "client-1",
                        "clientSecret": "secret-1",
                        "label": "My App",
                        "name": "my-app",
                        "description": null,
                        "status": "ACTIVE",
                        "url": null,
                        "proxyUrl": null,
                        "authorizationScopes": [],
                        "secret": "webhook-secret",
                        "publicKey": "public-key"
                    }
                }
            }));
        })
        .await;

    let input = create_application::MutationCreateApplicationInput {
        name: "my-app".to_owned(),
        label: "My App".to_owned(),
        description: None,
        url: None,
        proxy_url: None,
        organization_id: Some("org-1".to_owned()),
        authorization_scopes: Some(vec![]),
        distribution_type: None,
        redirect_uris: None,
    };
    let app = ApplicationClient::new(server.base_url(), "test-token")
        .create_application(input)
        .await
        .expect("create application")
        .expect("application present");

    mock.assert_async().await;
    assert_eq!(app.client_id.as_deref(), Some("client-1"));
    assert_eq!(app.secret.as_deref(), Some("webhook-secret"));
    assert_eq!(app.public_key.as_deref(), Some("public-key"));
}

#[tokio::test]
async fn generate_upload_url_sends_content_type_enum_and_returns_headers() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/v1/apps/app-registry-subgraph")
                .is_true(|req| {
                    let body = req.body_string();
                    body.contains("GenerateReleaseUploadUrl")
                        && body.contains(r#""contentType":"JS""#)
                });
            then.status(200).json_body(json!({
                "data": {
                    "generateReleaseUploadUrl": {
                        "uploadId": "up-1",
                        "url": "https://s3.example.com/upload",
                        "key": "key-1",
                        "expiresAt": "2026-01-01T00:00:00Z",
                        "maxSizeBytes": 1024,
                        "requiredHeaders": ["x-amz-signature:sig"]
                    }
                }
            }));
        })
        .await;

    let input = generate_release_upload_url::MutationGenerateReleaseUploadUrlInput {
        application_id: "app-1".to_owned(),
        release_id: "rel-1".to_owned(),
        content_type: generate_release_upload_url::UploadContentType::JS,
        target: None,
    };
    let upload = ApplicationClient::new(server.base_url(), "test-token")
        .generate_upload_url(input)
        .await
        .expect("generate upload url")
        .expect("upload response present");

    mock.assert_async().await;
    assert_eq!(upload.upload_id, "up-1");
    assert_eq!(upload.required_headers[0], "x-amz-signature:sig");
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
