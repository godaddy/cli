use bytes::Bytes;
use platform_app_client::{
    ActivateRelease, Application, ApplicationWithLatestRelease, ApplicationsList,
    ArchiveApplication, CreateApplication, CreateRelease, DisableApplication, EnableApplication,
    EnabledStoreApplications, GenerateReleaseUploadUrl, UpdateApplication, activate_release,
    application, application_with_latest_release, applications_list, archive_application,
    create_application, create_release, disable_application, enable_application,
    enabled_store_applications, generate_release_upload_url, update_application,
};
use reqwest::Client;
use serde_json::Value;

use crate::http::make_http_client;

const USER_AGENT: &str = concat!("godaddy-cli/", env!("CARGO_PKG_VERSION"));

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
    #[error("failed to construct App Registry client: {0}")]
    Build(#[from] platform_app_client::BuildError),
    #[error("file size {size} bytes exceeds maximum allowed ({max} bytes)")]
    TooLarge { size: u64, max: u64 },
    #[error("invalid upload header {0}")]
    InvalidHeader(String),
}

/// Reuses the existing `Http`/`Network`/`GraphQL` variants rather than
/// wrapping `platform_app_client::ClientError` in a new one — every caller
/// (and `impl From<ClientError> for GddyError` below) already matches on
/// those three shapes, and this keeps that mapping unchanged.
impl From<platform_app_client::ClientError> for ClientError {
    fn from(value: platform_app_client::ClientError) -> Self {
        match value {
            platform_app_client::ClientError::Http { status, body } => Self::Http { status, body },
            platform_app_client::ClientError::Network(e) => Self::Network(e),
            platform_app_client::ClientError::GraphQL(msg) => Self::GraphQL(msg),
        }
    }
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
            ClientError::Build(error) => {
                Self::config(format!("failed to construct App Registry client: {error}"))
                    .with_system("applications")
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
    http: Client,
    base_url: String,
    token: String,
}

#[allow(dead_code)]
impl ApplicationClient {
    pub fn new(base_url: impl Into<String>, token: impl Into<String>) -> Self {
        Self {
            http: make_http_client(),
            base_url: base_url.into(),
            token: token.into(),
        }
    }

    fn gql(&self) -> Result<platform_app_client::Client, ClientError> {
        Ok(platform_app_client::client_with_auth(
            &self.base_url,
            &format!("Bearer {}", self.token),
            USER_AGENT,
            &new_request_id(),
        )?)
    }

    /// Lists applications across every App Registry status.
    ///
    /// Always sends an explicit `status.in` of [`APPLICATION_STATUSES`]. Omitting
    /// the GraphQL `status` argument is *not* equivalent: the API defaults to
    /// ACTIVE-only.
    pub async fn list_applications(
        &self,
    ) -> Result<Vec<applications_list::ApplicationsListApplicationsEdgesNode>, ClientError> {
        use applications_list::ApplicationStatus;
        let statuses: Vec<ApplicationStatus> = APPLICATION_STATUSES
            .iter()
            .map(|s| match *s {
                "ACTIVE" => ApplicationStatus::ACTIVE,
                "ARCHIVED" => ApplicationStatus::ARCHIVED,
                "BLOCKED" => ApplicationStatus::BLOCKED,
                "INACTIVE" => ApplicationStatus::INACTIVE,
                "VERIFYING" => ApplicationStatus::VERIFYING,
                other => ApplicationStatus::Other(other.to_owned()),
            })
            .collect();
        let response = self
            .gql()?
            .send::<ApplicationsList>(applications_list::Variables {
                status: Some(applications_list::ApplicationStatusFilter {
                    eq: None,
                    in_: Some(statuses),
                    ne: None,
                }),
            })
            .await?;
        Ok(response
            .applications
            .and_then(|connection| connection.edges)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|edge| edge.node)
            .collect())
    }

    pub async fn get_application(
        &self,
        name: &str,
    ) -> Result<Option<application::ApplicationApplication>, ClientError> {
        let response = self
            .gql()?
            .send::<Application>(application::Variables {
                name: name.to_owned(),
            })
            .await?;
        Ok(response.application)
    }

    pub async fn update_application(
        &self,
        id: &str,
        input: update_application::MutationUpdateApplicationInput,
    ) -> Result<Option<update_application::UpdateApplicationUpdateApplication>, ClientError> {
        let response = self
            .gql()?
            .send::<UpdateApplication>(update_application::Variables {
                id: id.to_owned(),
                input,
            })
            .await?;
        Ok(response.update_application)
    }

    pub async fn create_release(
        &self,
        input: create_release::MutationCreateReleaseInput,
    ) -> Result<Option<create_release::CreateReleaseCreateRelease>, ClientError> {
        let response = self
            .gql()?
            .send::<CreateRelease>(create_release::Variables { input })
            .await?;
        Ok(response.create_release)
    }

    pub async fn activate_release(
        &self,
        application_id: &str,
        release_id: &str,
    ) -> Result<Option<activate_release::ActivateReleaseActivateRelease>, ClientError> {
        let response = self
            .gql()?
            .send::<ActivateRelease>(activate_release::Variables {
                application_id: application_id.to_owned(),
                release_id: release_id.to_owned(),
            })
            .await?;
        Ok(response.activate_release)
    }

    pub async fn enable_application(
        &self,
        input: enable_application::MutationEnableStoreApplicationInput,
    ) -> Result<Option<enable_application::EnableApplicationEnableStoreApplication>, ClientError>
    {
        let response = self
            .gql()?
            .send::<EnableApplication>(enable_application::Variables { input })
            .await?;
        Ok(response.enable_store_application)
    }

    pub async fn disable_application(
        &self,
        input: disable_application::MutationDisableStoreApplicationInput,
    ) -> Result<Option<disable_application::DisableApplicationDisableStoreApplication>, ClientError>
    {
        let response = self
            .gql()?
            .send::<DisableApplication>(disable_application::Variables { input })
            .await?;
        Ok(response.disable_store_application)
    }

    /// Lists applications enabled on a commerce store.
    ///
    /// Returns an empty list when nothing is enabled (not an error).
    pub async fn list_enabled_store_applications(
        &self,
        store_id: &str,
    ) -> Result<
        Vec<enabled_store_applications::EnabledStoreApplicationsEnabledStoreApplications>,
        ClientError,
    > {
        let response = self
            .gql()?
            .send::<EnabledStoreApplications>(enabled_store_applications::Variables {
                store_id: store_id.to_owned(),
            })
            .await?;
        Ok(response.enabled_store_applications.unwrap_or_default())
    }

    pub async fn archive_application(
        &self,
        id: &str,
    ) -> Result<Option<archive_application::ArchiveApplicationArchiveApplication>, ClientError>
    {
        let response = self
            .gql()?
            .send::<ArchiveApplication>(archive_application::Variables { id: id.to_owned() })
            .await?;
        Ok(response.archive_application)
    }

    pub async fn create_application(
        &self,
        input: create_application::MutationCreateApplicationInput,
    ) -> Result<Option<create_application::CreateApplicationCreateApplication>, ClientError> {
        let response = self
            .gql()?
            .send::<CreateApplication>(create_application::Variables { input })
            .await?;
        Ok(response.create_application)
    }

    pub async fn get_application_with_releases(
        &self,
        name: &str,
    ) -> Result<
        Option<application_with_latest_release::ApplicationWithLatestReleaseApplication>,
        ClientError,
    > {
        let response = self
            .gql()?
            .send::<ApplicationWithLatestRelease>(application_with_latest_release::Variables {
                name: name.to_owned(),
            })
            .await?;
        Ok(response.application)
    }

    pub async fn generate_upload_url(
        &self,
        input: generate_release_upload_url::MutationGenerateReleaseUploadUrlInput,
    ) -> Result<
        Option<generate_release_upload_url::GenerateReleaseUploadUrlGenerateReleaseUploadUrl>,
        ClientError,
    > {
        let response = self
            .gql()?
            .send::<GenerateReleaseUploadUrl>(generate_release_upload_url::Variables { input })
            .await?;
        Ok(response.generate_release_upload_url)
    }

    /// Upload an artifact to a presigned S3 URL.
    ///
    /// Validates the size up front, strips the unsigned `x-amz-meta-upload-id`
    /// header (it is not part of the S3 SigV4 signing string, so sending it can
    /// break the PUT), and retries transient failures — network errors and 5xx —
    /// with exponential backoff. 4xx responses are returned immediately.
    ///
    /// Not a GraphQL operation (a raw REST PUT to a presigned URL), so it stays
    /// hand-written here rather than moving into `platform-app-client`.
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
                .http
                .put(url)
                .body(bytes.clone())
                .headers(header_map.clone())
                .build()?;
            cli_engine::transport::debug_log_reqwest_request(&request);

            match self.http.execute(request).await {
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
