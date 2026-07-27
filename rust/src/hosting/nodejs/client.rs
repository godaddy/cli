use std::path::Path;

use reqwest::{Client, Method};
use serde_json::{Value, json};

use crate::application::client::make_http_client;

const BASE_PATH: &str = "/v1/hosting/nodejs";

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("HTTP error {status}: {body}")]
    Http { status: u16, body: String },
    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),
    #[error("failed to read {path}: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },
}

pub struct HostingClient {
    client: Client,
    base_url: String,
    token: String,
}

impl HostingClient {
    pub fn new(base_url: impl Into<String>, token: impl Into<String>) -> Self {
        Self {
            client: make_http_client(),
            base_url: base_url.into(),
            token: token.into(),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{BASE_PATH}{path}", self.base_url)
    }

    fn new_request_id() -> String {
        uuid::Uuid::new_v4().to_string()
    }

    async fn send_json(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, String)],
        body: Option<Value>,
    ) -> Result<Value, ClientError> {
        let mut req = self
            .client
            .request(method, self.url(path))
            .bearer_auth(&self.token)
            .header("x-request-id", Self::new_request_id());

        for (key, value) in query {
            req = req.query(&[(key, value)]);
        }

        if let Some(body) = body {
            req = req.json(&body);
        }

        let request = req.build()?;
        cli_engine::transport::debug_log_reqwest_request(&request);
        let resp = self.client.execute(request).await?;

        let status = resp.status();
        let headers = resp.headers().clone();
        let bytes = resp.bytes().await?;
        cli_engine::transport::debug_log_reqwest_response(status, &headers, &bytes);

        let status = status.as_u16();
        if status == 204 {
            return Ok(json!(null));
        }

        if !(200..300).contains(&status) {
            return Err(ClientError::Http {
                status,
                body: String::from_utf8_lossy(&bytes).into_owned(),
            });
        }

        if bytes.is_empty() {
            return Ok(json!(null));
        }

        serde_json::from_slice(&bytes).map_err(|e| ClientError::Http {
            status,
            body: format!(
                "invalid JSON response: {e} (body: {})",
                String::from_utf8_lossy(&bytes)
            ),
        })
    }

    pub async fn list_apps(&self) -> Result<Value, ClientError> {
        self.send_json(Method::GET, "/apps", &[], None).await
    }

    pub async fn get_app(&self, app_id: &str) -> Result<Value, ClientError> {
        self.send_json(Method::GET, &format!("/apps/{app_id}"), &[], None)
            .await
    }

    pub async fn create_app(&self, body: Value) -> Result<Value, ClientError> {
        self.send_json(Method::POST, "/apps", &[], Some(body)).await
    }

    pub async fn patch_app(&self, app_id: &str, body: Value) -> Result<Value, ClientError> {
        self.send_json(Method::PATCH, &format!("/apps/{app_id}"), &[], Some(body))
            .await
    }

    pub async fn delete_app(&self, app_id: &str) -> Result<Value, ClientError> {
        self.send_json(Method::DELETE, &format!("/apps/{app_id}"), &[], None)
            .await
    }

    pub async fn get_app_creation_status(&self, job_id: &str) -> Result<Value, ClientError> {
        self.send_json(Method::GET, &format!("/apps/jobs/{job_id}"), &[], None)
            .await
    }

    pub async fn list_deployments(
        &self,
        app_id: &str,
        limit: Option<u32>,
    ) -> Result<Value, ClientError> {
        let mut query = Vec::new();
        if let Some(limit) = limit {
            query.push(("limit", limit.to_string()));
        }
        self.send_json(
            Method::GET,
            &format!("/apps/{app_id}/deployments"),
            &query,
            None,
        )
        .await
    }

    pub async fn publish_app(&self, app_id: &str) -> Result<Value, ClientError> {
        self.send_json(
            Method::POST,
            &format!("/apps/{app_id}/deployments"),
            &[],
            Some(json!({})),
        )
        .await
    }

    pub async fn get_app_status(&self, app_id: &str) -> Result<Value, ClientError> {
        self.send_json(Method::GET, &format!("/apps/{app_id}/status"), &[], None)
            .await
    }

    pub async fn upload_source(&self, app_id: &str, zip_path: &Path) -> Result<Value, ClientError> {
        let form = reqwest::multipart::Form::new()
            .file("zipFile", zip_path)
            .await
            .map_err(|e| ClientError::Io {
                path: zip_path.display().to_string(),
                source: e,
            })?;

        let request = self
            .client
            .post(self.url(&format!("/apps/{app_id}/source")))
            .bearer_auth(&self.token)
            .header("x-request-id", Self::new_request_id())
            .multipart(form)
            .build()?;
        cli_engine::transport::debug_log_reqwest_request(&request);
        let resp = self.client.execute(request).await?;

        let status = resp.status();
        let headers = resp.headers().clone();
        let bytes = resp.bytes().await?;
        cli_engine::transport::debug_log_reqwest_response(status, &headers, &bytes);

        let status = status.as_u16();
        if !(200..300).contains(&status) {
            return Err(ClientError::Http {
                status,
                body: String::from_utf8_lossy(&bytes).into_owned(),
            });
        }

        serde_json::from_slice(&bytes).map_err(|e| ClientError::Http {
            status,
            body: format!(
                "invalid JSON response: {e} (body: {})",
                String::from_utf8_lossy(&bytes)
            ),
        })
    }

    pub async fn get_source_upload_status(
        &self,
        app_id: &str,
        job_id: &str,
    ) -> Result<Value, ClientError> {
        self.send_json(
            Method::GET,
            &format!("/apps/{app_id}/source/status"),
            &[("jobId", job_id.to_owned())],
            None,
        )
        .await
    }

    pub async fn list_secrets(
        &self,
        app_id: &str,
        variant: Option<&str>,
    ) -> Result<Value, ClientError> {
        let mut query = Vec::new();
        if let Some(variant) = variant {
            query.push(("variant", variant.to_owned()));
        }
        self.send_json(
            Method::GET,
            &format!("/apps/{app_id}/secrets"),
            &query,
            None,
        )
        .await
    }

    pub async fn update_secrets(&self, app_id: &str, body: Value) -> Result<Value, ClientError> {
        self.send_json(
            Method::POST,
            &format!("/apps/{app_id}/secrets"),
            &[],
            Some(body),
        )
        .await
    }

    pub async fn get_github_status(&self, app_id: &str) -> Result<Value, ClientError> {
        self.send_json(
            Method::GET,
            &format!("/apps/{app_id}/github/status"),
            &[],
            None,
        )
        .await
    }

    pub async fn list_github_repos(
        &self,
        app_id: &str,
        per_page: Option<u32>,
        sort: Option<&str>,
    ) -> Result<Value, ClientError> {
        let mut query = Vec::new();
        if let Some(per_page) = per_page {
            query.push(("per_page", per_page.to_string()));
        }
        if let Some(sort) = sort {
            query.push(("sort", sort.to_owned()));
        }
        self.send_json(
            Method::GET,
            &format!("/apps/{app_id}/github/repos"),
            &query,
            None,
        )
        .await
    }

    pub async fn list_github_branches(
        &self,
        app_id: &str,
        owner: &str,
        repo: &str,
        per_page: Option<u32>,
    ) -> Result<Value, ClientError> {
        let mut query = vec![("owner", owner.to_owned()), ("repo", repo.to_owned())];
        if let Some(per_page) = per_page {
            query.push(("per_page", per_page.to_string()));
        }
        self.send_json(
            Method::GET,
            &format!("/apps/{app_id}/github/branches"),
            &query,
            None,
        )
        .await
    }

    pub async fn start_git_import(&self, app_id: &str, body: Value) -> Result<Value, ClientError> {
        self.send_json(
            Method::POST,
            &format!("/apps/{app_id}/source/git"),
            &[],
            Some(body),
        )
        .await
    }

    pub async fn get_git_import_status(
        &self,
        app_id: &str,
        job_id: &str,
    ) -> Result<Value, ClientError> {
        self.send_json(
            Method::GET,
            &format!("/apps/{app_id}/source/git/status"),
            &[("jobId", job_id.to_owned())],
            None,
        )
        .await
    }

    pub async fn get_logs(
        &self,
        app_id: &str,
        target: &str,
        source: &str,
        since: &str,
        lines: Option<u32>,
    ) -> Result<Value, ClientError> {
        let mut query = vec![
            ("target", target.to_owned()),
            ("source", source.to_owned()),
            ("since", since.to_owned()),
        ];
        if let Some(lines) = lines {
            query.push(("lines", lines.to_string()));
        }
        self.send_json(Method::GET, &format!("/apps/{app_id}/logs"), &query, None)
            .await
    }
}

#[cfg(test)]
mod tests {
    use httpmock::prelude::*;
    use serde_json::json;

    use super::*;

    fn client(base_url: &str) -> HostingClient {
        HostingClient::new(base_url, "test-token")
    }

    #[tokio::test]
    async fn list_apps_sends_bearer_auth() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/v1/hosting/nodejs/apps")
                    .header("authorization", "Bearer test-token");
                then.status(200).json_body(json!({ "apps": [] }));
            })
            .await;

        let body = client(&server.base_url())
            .list_apps()
            .await
            .expect("list apps");

        mock.assert_async().await;
        assert_eq!(body["apps"], json!([]));
    }

    #[tokio::test]
    async fn create_app_posts_json_body() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v1/hosting/nodejs/apps")
                    .json_body(json!({ "name": "demo" }));
                then.status(202).json_body(json!({
                    "job": { "id": "job-1", "status": "pending" }
                }));
            })
            .await;

        let body = client(&server.base_url())
            .create_app(json!({ "name": "demo" }))
            .await
            .expect("create app");

        mock.assert_async().await;
        assert_eq!(body["job"]["id"], "job-1");
    }

    #[tokio::test]
    async fn upload_source_sends_multipart() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v1/hosting/nodejs/apps/app-1/source")
                    .matches(|req| {
                        req.body
                            .as_ref()
                            .map(|b| String::from_utf8_lossy(b).contains("zipFile"))
                            .unwrap_or(false)
                    });
                then.status(200).json_body(json!({ "jobId": "upload-1" }));
            })
            .await;

        let zip_path = std::env::temp_dir().join("gddy-hosting-upload-test.zip");
        std::fs::write(&zip_path, b"zip bytes").expect("write temp zip");

        let body = client(&server.base_url())
            .upload_source("app-1", &zip_path)
            .await
            .expect("upload source");

        let _ = std::fs::remove_file(&zip_path);

        mock.assert_async().await;
        assert_eq!(body["jobId"], "upload-1");
    }

    #[tokio::test]
    async fn get_github_status_sends_bearer_auth() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/v1/hosting/nodejs/apps/app-1/github/status")
                    .header("authorization", "Bearer test-token");
                then.status(200).json_body(json!({ "connected": true }));
            })
            .await;

        let body = client(&server.base_url())
            .get_github_status("app-1")
            .await
            .expect("get github status");

        mock.assert_async().await;
        assert_eq!(body["connected"], json!(true));
    }

    #[tokio::test]
    async fn list_github_repos_sends_query_params() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/v1/hosting/nodejs/apps/app-1/github/repos")
                    .query_param("per_page", "10")
                    .query_param("sort", "updated");
                then.status(200).json_body(json!({ "repos": [] }));
            })
            .await;

        let body = client(&server.base_url())
            .list_github_repos("app-1", Some(10), Some("updated"))
            .await
            .expect("list github repos");

        mock.assert_async().await;
        assert_eq!(body["repos"], json!([]));
    }

    #[tokio::test]
    async fn list_github_branches_sends_owner_and_repo() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/v1/hosting/nodejs/apps/app-1/github/branches")
                    .query_param("owner", "acme")
                    .query_param("repo", "my-app");
                then.status(200).json_body(json!({ "branches": [] }));
            })
            .await;

        let body = client(&server.base_url())
            .list_github_branches("app-1", "acme", "my-app", None)
            .await
            .expect("list github branches");

        mock.assert_async().await;
        assert_eq!(body["branches"], json!([]));
    }

    #[tokio::test]
    async fn start_git_import_posts_json_body() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v1/hosting/nodejs/apps/app-1/source/git")
                    .json_body(json!({ "branch": "main", "repoFullName": "acme/my-app" }));
                then.status(202)
                    .json_body(json!({ "jobId": "git-1", "status": "in_progress" }));
            })
            .await;

        let body = client(&server.base_url())
            .start_git_import(
                "app-1",
                json!({ "branch": "main", "repoFullName": "acme/my-app" }),
            )
            .await
            .expect("start git import");

        mock.assert_async().await;
        assert_eq!(body["jobId"], "git-1");
    }

    #[tokio::test]
    async fn publish_app_sends_empty_json_body_and_content_length() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v1/hosting/nodejs/apps/app-1/deployments")
                    .header("content-type", "application/json")
                    .header_exists("content-length")
                    .json_body(json!({}));
                then.status(200).json_body(json!({
                    "deploymentId": "dep-1",
                    "status": "pending"
                }));
            })
            .await;

        let body = client(&server.base_url())
            .publish_app("app-1")
            .await
            .expect("publish app");

        mock.assert_async().await;
        assert_eq!(body["deploymentId"], "dep-1");
    }

    #[tokio::test]
    async fn get_git_import_status_sends_job_id() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/v1/hosting/nodejs/apps/app-1/source/git/status")
                    .query_param("jobId", "git-1");
                then.status(200).json_body(json!({ "status": "complete" }));
            })
            .await;

        let body = client(&server.base_url())
            .get_git_import_status("app-1", "git-1")
            .await
            .expect("get git import status");

        mock.assert_async().await;
        assert_eq!(body["status"], "complete");
    }
}
