use bytes::Bytes;
use reqwest::Client;
use serde_json::{Value, json};

use crate::http::{make_http_client, make_upload_http_client};

const GRAPHQL_PATH: &str = "/v1/apps/app-registry-subgraph";

/// App Registry `ApplicationStatus` enum values (see app-registry-api GraphQL schema).
///
/// The `applications` query defaults to ACTIVE-only when `status` is omitted, so
/// callers that want every app must pass an explicit `status.in` containing these.
pub const APPLICATION_STATUSES: &[&str] =
    &["ACTIVE", "ARCHIVED", "BLOCKED", "INACTIVE", "VERIFYING"];

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
    /// Built on first upload so commands that never upload don't pay for it.
    /// See [`crate::http::upload_timeouts`] for why uploads need their own.
    upload_client: std::sync::OnceLock<Client>,
    base_url: String,
    token: String,
}

#[allow(dead_code)]
impl ApplicationClient {
    pub fn new(base_url: impl Into<String>, token: impl Into<String>) -> Self {
        Self {
            client: make_http_client(),
            upload_client: std::sync::OnceLock::new(),
            base_url: base_url.into(),
            token: token.into(),
        }
    }

    /// Builds a client with explicit HTTP clients, so a test can give the API
    /// client a tight timeout and prove uploads don't use it.
    #[cfg(test)]
    fn with_clients(
        base_url: impl Into<String>,
        token: impl Into<String>,
        client: Client,
        upload_client: Client,
    ) -> Self {
        Self {
            client,
            upload_client: upload_client.into(),
            base_url: base_url.into(),
            token: token.into(),
        }
    }

    fn upload_client(&self) -> &Client {
        self.upload_client.get_or_init(make_upload_http_client)
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

    /// Lists applications across every App Registry status.
    ///
    /// Always sends an explicit `status.in` of [`APPLICATION_STATUSES`]. Omitting
    /// the GraphQL `status` argument is *not* equivalent: the API defaults to
    /// ACTIVE-only.
    pub async fn list_applications(&self) -> Result<Value, ClientError> {
        let data = self
            .query(json!({
                "query": "query ApplicationsList($status: ApplicationStatusFilter) { applications(status: $status) { edges { node { id label name description status url proxyUrl } } } }",
                "variables": { "status": { "in": APPLICATION_STATUSES } }
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
            "query": "mutation UpdateApplication($id: String!, $input: MutationUpdateApplicationInput!) { updateApplication(id: $id, input: $input) { id clientId label name description status url proxyUrl authorizationScopes redirectUris } }",
            "variables": { "id": id, "input": input }
        }))
        .await
    }

    pub async fn create_release(&self, input: Value) -> Result<Value, ClientError> {
        self.query(json!({
            "query": "mutation CreateRelease($input: MutationCreateReleaseInput!) { createRelease(input: $input) { id version description createdAt uiExtensions { id name handle type source target } nativeExtensions { id name packageName contact platform } settings { id groupSlug appSettingSlug entryPath capabilities order title } } }",
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

    /// Lists applications enabled on a commerce store.
    ///
    /// Returns an empty JSON array when nothing is enabled (not an error).
    /// `storeId` is sent as a GraphQL variable only — same pattern as
    /// [`Self::enable_application`] / [`Self::disable_application`].
    pub async fn list_enabled_store_applications(
        &self,
        store_id: &str,
    ) -> Result<Value, ClientError> {
        let data = self
            .query(json!({
                "query": "query EnabledStoreApplications($storeId: String!) { enabledStoreApplications(storeId: $storeId) { id name label status release { id version } } }",
                "variables": { "storeId": store_id }
            }))
            .await?;
        Ok(data
            .get("enabledStoreApplications")
            .cloned()
            .unwrap_or_else(|| json!([])))
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
            "query": "mutation CreateApplication($input: MutationCreateApplicationInput!) { createApplication(input: $input) { id clientId clientSecret label name description status url proxyUrl authorizationScopes redirectUris secret publicKey } }",
            "variables": { "input": input }
        }))
        .await
    }

    pub async fn get_application_with_releases(&self, name: &str) -> Result<Value, ClientError> {
        self.query(json!({
            "query": "query ApplicationWithLatestRelease($name: String!) { application(name: $name) { id label name description status url proxyUrl authorizationScopes redirectUris clientId releases(first: 1, orderBy: { createdAt: DESC }) { edges { node { id version description createdAt subscriptions { name url events } } } } } }",
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
        // One client builds and executes every attempt: the executing client's
        // timeouts are the ones that apply.
        let client = self.upload_client();

        for attempt in 1..=opts.max_attempts {
            let request = client
                .put(url)
                .body(bytes.clone())
                .headers(header_map.clone())
                .build()?;
            cli_engine::transport::debug_log_reqwest_request(&request);

            match client.execute(request).await {
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

#[cfg(test)]
#[path = "client_tests.rs"]
mod tests;
