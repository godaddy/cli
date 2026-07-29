use bytes::Bytes;
use reqwest::Client;
use serde_json::{Value, json};

const GRAPHQL_PATH: &str = "/v1/apps/app-registry-subgraph";
const USER_AGENT: &str = concat!("godaddy-cli/", env!("CARGO_PKG_VERSION"));

/// Builds a reqwest Client with the standard GoDaddy CLI User-Agent.
pub fn make_http_client() -> Client {
    Client::builder()
        .user_agent(USER_AGENT)
        .build()
        .expect("failed to build HTTP client")
}

fn new_request_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("HTTP error {status}: {body}")]
    Http { status: u16, body: String },
    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),
    #[error("GraphQL errors: {0}")]
    GraphQL(String),
    #[error("file size {size} bytes exceeds maximum allowed ({max} bytes)")]
    TooLarge { size: u64, max: u64 },
    #[error("invalid upload header {0}")]
    InvalidHeader(String),
}

impl From<ClientError> for crate::error::GddyError {
    fn from(value: ClientError) -> Self {
        match value {
            ClientError::Http { status, body } => Self::from_http(status, body, "applications"),
            ClientError::Network(e) => {
                Self::network(format!("network error: {e}")).with_system("applications")
            }
            ClientError::GraphQL(msg) => {
                Self::from_graphql(format!("GraphQL errors: {msg}"), "applications")
            }
            ClientError::TooLarge { size, max } => Self::validation(format!(
                "file size {size} bytes exceeds maximum allowed ({max} bytes)"
            ))
            .with_system("applications"),
            ClientError::InvalidHeader(name) => {
                Self::validation(format!("invalid upload header {name}"))
                    .with_system("applications")
            }
        }
    }
}

/// Result of a successful artifact upload.
#[derive(Debug, Clone)]
pub struct UploadResult {
    pub upload_id: String,
    pub etag: Option<String>,
    pub status: u16,
    pub size_bytes: u64,
}

/// Retry/backoff tuning for [`ApplicationClient::upload_artifact`].
#[derive(Debug, Clone)]
pub struct UploadOptions {
    pub max_attempts: u32,
    pub base_delay_ms: u64,
}

impl Default for UploadOptions {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base_delay_ms: 250,
        }
    }
}

pub struct ApplicationClient {
    client: Client,
    base_url: String,
    token: String,
}

#[allow(dead_code)]
impl ApplicationClient {
    pub fn new(base_url: impl Into<String>, token: impl Into<String>) -> Self {
        Self {
            client: make_http_client(),
            base_url: base_url.into(),
            token: token.into(),
        }
    }

    async fn query(&self, body: Value) -> Result<Value, ClientError> {
        let url = format!("{}{}", self.base_url, GRAPHQL_PATH);
        let request = self
            .client
            .post(&url)
            .bearer_auth(&self.token)
            .header("x-request-id", new_request_id())
            .json(&body)
            .build()?;
        cli_engine::transport::debug_log_reqwest_request(&request);
        let resp = self.client.execute(request).await?;

        let status = resp.status();
        let headers = resp.headers().clone();
        let bytes = resp.bytes().await?;
        cli_engine::transport::debug_log_reqwest_response(status, &headers, &bytes);

        if !status.is_success() {
            let text = String::from_utf8_lossy(&bytes).into_owned();
            return Err(ClientError::Http {
                status: status.as_u16(),
                body: text,
            });
        }

        let payload: Value = serde_json::from_slice(&bytes).map_err(|e| ClientError::Http {
            status: status.as_u16(),
            body: format!(
                "invalid JSON response: {e} (body: {})",
                String::from_utf8_lossy(&bytes)
            ),
        })?;
        if let Some(errors) = payload.get("errors") {
            return Err(ClientError::GraphQL(errors.to_string()));
        }
        Ok(payload["data"].clone())
    }

    pub async fn list_applications(&self) -> Result<Value, ClientError> {
        let data = self.query(json!({
            "query": "query ApplicationsList { applications { edges { node { id label name description status url proxyUrl } } } }"
        }))
        .await?;
        let nodes: Vec<Value> = data["applications"]["edges"]
            .as_array()
            .map(|edges| {
                edges
                    .iter()
                    .filter_map(|e| {
                        let node = &e["node"];
                        if node.is_null() {
                            None
                        } else {
                            Some(node.clone())
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(json!(nodes))
    }

    pub async fn get_application(&self, name: &str) -> Result<Value, ClientError> {
        self.query(json!({
            "query": "query Application($name: String!) { application(name: $name) { id label name description status url proxyUrl } }",
            "variables": { "name": name }
        }))
        .await
    }

    pub async fn update_application(&self, id: &str, input: Value) -> Result<Value, ClientError> {
        self.query(json!({
            "query": "mutation UpdateApplication($id: String!, $input: MutationUpdateApplicationInput!) { updateApplication(id: $id, input: $input) { id clientId label name description status url proxyUrl authorizationScopes } }",
            "variables": { "id": id, "input": input }
        }))
        .await
    }

    pub async fn create_release(&self, input: Value) -> Result<Value, ClientError> {
        self.query(json!({
            "query": "mutation CreateRelease($input: MutationCreateReleaseInput!) { createRelease(input: $input) { id version description createdAt uiExtensions { id name handle type source target } } }",
            "variables": { "input": input }
        }))
        .await
    }

    pub async fn activate_release(
        &self,
        application_id: &str,
        release_id: &str,
    ) -> Result<Value, ClientError> {
        self.query(json!({
            "query": "mutation ActivateRelease($applicationId: ID!, $releaseId: ID!) { activateRelease(applicationId: $applicationId, releaseId: $releaseId) { id version description status activatedAt createdAt updatedAt } }",
            "variables": { "applicationId": application_id, "releaseId": release_id }
        }))
        .await
    }

    pub async fn enable_application(&self, input: Value) -> Result<Value, ClientError> {
        self.query(json!({
            "query": "mutation EnableApplication($input: MutationEnableStoreApplicationInput!) { enableStoreApplication(input: $input) { id } }",
            "variables": { "input": input }
        }))
        .await
    }

    pub async fn disable_application(&self, input: Value) -> Result<Value, ClientError> {
        self.query(json!({
            "query": "mutation DisableApplication($input: MutationDisableStoreApplicationInput!) { disableStoreApplication(input: $input) { id } }",
            "variables": { "input": input }
        }))
        .await
    }

    pub async fn archive_application(&self, id: &str) -> Result<Value, ClientError> {
        self.query(json!({
            "query": "mutation ArchiveApplication($id: String!) { archiveApplication(id: $id) { id label name status createdAt archivedAt } }",
            "variables": { "id": id }
        }))
        .await
    }

    pub async fn create_application(&self, input: Value) -> Result<Value, ClientError> {
        self.query(json!({
            "query": "mutation CreateApplication($input: MutationCreateApplicationInput!) { createApplication(input: $input) { id clientId clientSecret label name description status url proxyUrl authorizationScopes secret publicKey } }",
            "variables": { "input": input }
        }))
        .await
    }

    pub async fn get_application_with_releases(&self, name: &str) -> Result<Value, ClientError> {
        self.query(json!({
            "query": "query ApplicationWithLatestRelease($name: String!) { application(name: $name) { id label name description status url proxyUrl authorizationScopes releases(first: 1, orderBy: { createdAt: DESC }) { edges { node { id version description createdAt } } } } }",
            "variables": { "name": name }
        }))
        .await
    }

    pub async fn generate_upload_url(&self, input: Value) -> Result<Value, ClientError> {
        self.query(json!({
            "query": "mutation GenerateReleaseUploadUrl($input: MutationGenerateReleaseUploadUrlInput!) { generateReleaseUploadUrl(input: $input) { uploadId url key expiresAt maxSizeBytes requiredHeaders } }",
            "variables": { "input": input }
        }))
        .await
    }

    /// Upload an artifact to a presigned S3 URL.
    ///
    /// Validates the size up front, strips the unsigned `x-amz-meta-upload-id`
    /// header (it is not part of the S3 SigV4 signing string, so sending it can
    /// break the PUT), and retries transient failures — network errors and 5xx —
    /// with exponential backoff. 4xx responses are returned immediately.
    pub async fn upload_artifact(
        &self,
        url: &str,
        upload_id: &str,
        headers: &Value,
        max_size_bytes: Option<u64>,
        bytes: Bytes,
        opts: UploadOptions,
    ) -> Result<UploadResult, ClientError> {
        let size_bytes = bytes.len() as u64;
        if let Some(max) = max_size_bytes
            && size_bytes > max
        {
            return Err(ClientError::TooLarge {
                size: size_bytes,
                max,
            });
        }

        // Skip the unsigned x-amz-meta-upload-id; fail fast on a malformed header (avoids an opaque S3 403).
        let mut header_map = reqwest::header::HeaderMap::new();
        for (k, v) in headers.as_object().into_iter().flatten() {
            if k.eq_ignore_ascii_case("x-amz-meta-upload-id") {
                continue;
            }
            let name = reqwest::header::HeaderName::from_bytes(k.as_bytes())
                .map_err(|e| ClientError::InvalidHeader(format!("name {k:?}: {e}")))?;
            let value = v.as_str().ok_or_else(|| {
                ClientError::InvalidHeader(format!("{k:?} value is not a string"))
            })?;
            let value = reqwest::header::HeaderValue::from_str(value)
                .map_err(|e| ClientError::InvalidHeader(format!("value for {k:?}: {e}")))?;
            header_map.insert(name, value);
        }

        let mut last_error: Option<ClientError> = None;

        for attempt in 1..=opts.max_attempts {
            let request = self
                .client
                .put(url)
                .body(bytes.clone())
                .headers(header_map.clone())
                .build()?;
            cli_engine::transport::debug_log_reqwest_request(&request);

            match self.client.execute(request).await {
                Ok(resp) => {
                    let status = resp.status();
                    let resp_headers = resp.headers().clone();
                    let etag = resp_headers
                        .get(reqwest::header::ETAG)
                        .and_then(|v| v.to_str().ok())
                        .map(|s| s.to_owned());
                    // Body-read failure is non-fatal — status is authoritative; body is only for the snippet.
                    let body = resp.bytes().await.unwrap_or_default();
                    cli_engine::transport::debug_log_reqwest_response(status, &resp_headers, &body);

                    if status.is_success() {
                        let result = UploadResult {
                            upload_id: upload_id.to_owned(),
                            etag,
                            status: status.as_u16(),
                            size_bytes,
                        };
                        tracing::debug!(
                            upload_id = %result.upload_id,
                            status = result.status,
                            etag = ?result.etag,
                            size_bytes = result.size_bytes,
                            attempt,
                            "artifact upload succeeded"
                        );
                        return Ok(result);
                    }

                    let snippet: String =
                        String::from_utf8_lossy(&body).chars().take(200).collect();
                    if !status.is_server_error() {
                        return Err(ClientError::Http {
                            status: status.as_u16(),
                            body: snippet,
                        });
                    }
                    tracing::warn!(
                        %upload_id,
                        status = status.as_u16(),
                        attempt,
                        max_attempts = opts.max_attempts,
                        error_snippet = %snippet,
                        "artifact upload failed with server error, retrying"
                    );
                    last_error = Some(ClientError::Http {
                        status: status.as_u16(),
                        body: snippet,
                    });
                }
                Err(e) => {
                    tracing::warn!(
                        %upload_id,
                        attempt,
                        max_attempts = opts.max_attempts,
                        error = %e,
                        "artifact upload failed with network error, retrying"
                    );
                    last_error = Some(ClientError::Network(e));
                }
            }

            if attempt < opts.max_attempts {
                let delay = opts.base_delay_ms * 3u64.pow(attempt - 1);
                tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
            }
        }

        Err(last_error.unwrap_or_else(|| ClientError::Http {
            status: 0,
            body: format!("upload failed after {} attempts", opts.max_attempts),
        }))
    }
}

pub fn api_url_for_env(env: &str) -> String {
    crate::environments::resolve(env)
        .or_else(|_| crate::environments::resolve(crate::environments::DEFAULT_ENV))
        .map(|e| e.api_url)
        // Never return an empty base URL (e.g. if a malformed local config makes
        // even the default fail to resolve) — fall back to the built-in default.
        .unwrap_or_else(|_| crate::environments::default_api_url().to_owned())
}

#[cfg(test)]
mod tests {
    use httpmock::prelude::*;

    use super::*;

    #[test]
    fn api_url_for_builtins_resolve_to_a_url() {
        // The exact host mapping is covered deterministically in
        // `environments::tests`. Here we only assert the built-ins resolve to a
        // URL — a dev machine may legitimately override a built-in's URL via
        // env var / local config, so don't hard-code the host.
        for env in ["prod", "ote"] {
            let url = api_url_for_env(env);
            assert!(url.contains("://"), "{env} -> {url:?}");
        }
    }

    #[test]
    fn api_url_for_unknown_env_falls_back_to_default_and_is_never_empty() {
        // Unknown env resolves to the default environment's URL (never empty).
        let url = api_url_for_env("definitely-not-a-real-env-xyz");
        assert!(!url.is_empty());
        // Don't hard-code the scheme: a built-in's URL is overridable (a dev
        // may point the default at an http:// local proxy).
        assert!(url.contains("://"), "{url:?}");
    }

    #[tokio::test]
    async fn activate_release_posts_mutation_with_ids() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v1/apps/app-registry-subgraph")
                    .header("authorization", "Bearer test-token")
                    .matches(|req| {
                        req.body
                            .as_ref()
                            .map(|b| {
                                let body = String::from_utf8_lossy(b);
                                body.contains("activateRelease")
                                    && body.contains("app-123")
                                    && body.contains("rel-456")
                            })
                            .unwrap_or(false)
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

    #[tokio::test]
    async fn update_application_promotes_to_active() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v1/apps/app-registry-subgraph")
                    .matches(|req| {
                        req.body
                            .as_ref()
                            .map(|b| {
                                let body = String::from_utf8_lossy(b);
                                body.contains("updateApplication")
                                    && body.contains(r#""status":"ACTIVE""#)
                            })
                            .unwrap_or(false)
                    });
                then.status(200).json_body(json!({
                    "data": { "updateApplication": { "id": "app-1", "status": "ACTIVE" } }
                }));
            })
            .await;

        let data = ApplicationClient::new(server.base_url(), "test-token")
            .update_application("app-1", json!({ "status": "ACTIVE" }))
            .await
            .expect("update application");

        mock.assert_async().await;
        assert_eq!(data["updateApplication"]["status"], "ACTIVE");
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
        assert_eq!(mock.hits_async().await, 0);
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
        assert_eq!(mock.hits_async().await, 1);
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
        assert_eq!(mock.hits_async().await, 3);
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
                    .matches(|req| {
                        !req.headers
                            .iter()
                            .flatten()
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
        assert_eq!(mock.hits_async().await, 0);
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

        mock.assert_hits_async(2).await;
    }
}
