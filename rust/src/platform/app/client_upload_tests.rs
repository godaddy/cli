use httpmock::prelude::*;
use serde_json::json;

use super::*;

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

    // Control: a client with an API-style read timeout really is that tight
    // against this server.
    assert!(
        crate::http::make_api_client_with_read_timeout(std::time::Duration::from_millis(100))
            .put(server.url("/artifact"))
            .body("x")
            .send()
            .await
            .is_err(),
        "the API client should time out against the slow server"
    );
    let client = ApplicationClient::with_upload_client(
        server.base_url(),
        "test-token",
        make_storage_upload_http_client(),
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
