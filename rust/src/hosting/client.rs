use reqwest::{Client, Method};
use serde_json::{Value, json};

use crate::application::client::make_http_client;

const BASE_PATH: &str = "/v1/hosting";

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

impl From<ClientError> for crate::error::GddyError {
    fn from(value: ClientError) -> Self {
        match value {
            ClientError::Http { status, body } => Self::from_http(status, body, "hosting"),
            ClientError::Network(e) => {
                Self::network(format!("network error: {e}")).with_system("hosting")
            }
            ClientError::Io { path, source } => {
                Self::validation(format!("failed to read {path}: {source}")).with_system("hosting")
            }
        }
    }
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

    // JSON Patch (RFC 6902) requires application/json-patch+json, which reqwest's
    // .json() won't set. Serialize manually and force the content-type header.
    async fn send_patch(&self, path: &str, body: Value) -> Result<Value, ClientError> {
        let body_str = serde_json::to_string(&body).map_err(|e| ClientError::Http {
            status: 0,
            body: format!("failed to serialize patch: {e}"),
        })?;

        let request = self
            .client
            .request(Method::PATCH, self.url(path))
            .bearer_auth(&self.token)
            .header("x-request-id", Self::new_request_id())
            .header("content-type", "application/json-patch+json")
            .body(body_str)
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

    pub async fn list_apps(
        &self,
        app_type: &str,
        page_token: Option<&str>,
        limit: Option<u32>,
    ) -> Result<Value, ClientError> {
        let mut query = vec![("appType", app_type.to_owned())];
        if let Some(token) = page_token {
            query.push(("pageToken", token.to_owned()));
        }
        if let Some(limit) = limit {
            query.push(("limit", limit.to_string()));
        }
        self.send_json(Method::GET, "/apps", &query, None).await
    }

    pub async fn get_app(&self, app_id: &str) -> Result<Value, ClientError> {
        self.send_json(Method::GET, &format!("/apps/{app_id}"), &[], None)
            .await
    }

    pub async fn create_app(&self, app_type: &str, body: Value) -> Result<Value, ClientError> {
        let query = [("appType", app_type.to_owned())];
        self.send_json(Method::POST, "/apps", &query, Some(body))
            .await
    }

    pub async fn update_app(&self, app_id: &str, patch: Value) -> Result<Value, ClientError> {
        self.send_patch(&format!("/apps/{app_id}"), patch).await
    }

    pub async fn delete_app(&self, app_id: &str) -> Result<Value, ClientError> {
        self.send_json(Method::DELETE, &format!("/apps/{app_id}"), &[], None)
            .await
    }

    pub async fn get_app_status(&self, app_id: &str) -> Result<Value, ClientError> {
        self.send_json(Method::GET, &format!("/apps/{app_id}/status"), &[], None)
            .await
    }

    pub async fn restart_app(&self, app_id: &str, variant: &str) -> Result<Value, ClientError> {
        self.send_json(
            Method::POST,
            &format!("/apps/{app_id}/restarts"),
            &[],
            Some(json!({ "variant": variant })),
        )
        .await
    }

    pub async fn list_deployments(
        &self,
        app_id: &str,
        page_token: Option<&str>,
        limit: Option<u32>,
    ) -> Result<Value, ClientError> {
        let mut query: Vec<(&str, String)> = Vec::new();
        if let Some(t) = page_token {
            query.push(("pageToken", t.to_owned()));
        }
        if let Some(l) = limit {
            query.push(("limit", l.to_string()));
        }
        self.send_json(
            Method::GET,
            &format!("/apps/{app_id}/deployments"),
            &query,
            None,
        )
        .await
    }

    pub async fn get_deployment(
        &self,
        app_id: &str,
        deployment_id: &str,
    ) -> Result<Value, ClientError> {
        self.send_json(
            Method::GET,
            &format!("/apps/{app_id}/deployments/{deployment_id}"),
            &[],
            None,
        )
        .await
    }

    pub async fn create_deployment(&self, app_id: &str) -> Result<Value, ClientError> {
        self.send_json(
            Method::POST,
            &format!("/apps/{app_id}/deployments"),
            &[],
            None,
        )
        .await
    }

    pub async fn get_operation(&self, operation_id: &str) -> Result<Value, ClientError> {
        self.send_json(
            Method::GET,
            &format!("/app-operations/{operation_id}"),
            &[],
            None,
        )
        .await
    }

    pub async fn create_import(
        &self,
        app_id: &str,
        repo: &str,
        branch: &str,
    ) -> Result<Value, ClientError> {
        self.send_json(
            Method::POST,
            &format!("/apps/{app_id}/imports"),
            &[],
            Some(json!({ "repositoryFullName": repo, "branch": branch })),
        )
        .await
    }

    pub async fn create_import_zip(
        &self,
        app_id: &str,
        zip_path: &std::path::Path,
    ) -> Result<Value, ClientError> {
        let form = reqwest::multipart::Form::new()
            .file("file", zip_path)
            .await
            .map_err(|e| ClientError::Io {
                path: zip_path.display().to_string(),
                source: e,
            })?;

        let request = self
            .client
            .post(self.url(&format!("/apps/{app_id}/imports")))
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

    pub async fn get_import(&self, app_id: &str, import_id: &str) -> Result<Value, ClientError> {
        self.send_json(
            Method::GET,
            &format!("/apps/{app_id}/imports/{import_id}"),
            &[],
            None,
        )
        .await
    }

    pub async fn get_github_connection(&self) -> Result<Value, ClientError> {
        self.send_json(Method::GET, "/settings/github/connection", &[], None)
            .await
    }

    pub async fn list_github_repos(
        &self,
        page_token: Option<&str>,
        limit: Option<u32>,
    ) -> Result<Value, ClientError> {
        let mut query: Vec<(&str, String)> = Vec::new();
        if let Some(t) = page_token {
            query.push(("pageToken", t.to_owned()));
        }
        if let Some(l) = limit {
            query.push(("limit", l.to_string()));
        }
        self.send_json(Method::GET, "/settings/github/repositories", &query, None)
            .await
    }

    pub async fn list_github_branches(
        &self,
        owner: &str,
        repo: &str,
        page_token: Option<&str>,
        limit: Option<u32>,
    ) -> Result<Value, ClientError> {
        let mut query: Vec<(&str, String)> = Vec::new();
        if let Some(t) = page_token {
            query.push(("pageToken", t.to_owned()));
        }
        if let Some(l) = limit {
            query.push(("limit", l.to_string()));
        }
        self.send_json(
            Method::GET,
            &format!("/settings/github/repositories/{owner}/{repo}/branches"),
            &query,
            None,
        )
        .await
    }

    pub async fn list_secrets(
        &self,
        app_id: &str,
        variant: Option<&str>,
    ) -> Result<Value, ClientError> {
        let mut query: Vec<(&str, String)> = Vec::new();
        if let Some(v) = variant {
            query.push(("variant", v.to_owned()));
        }
        self.send_json(
            Method::GET,
            &format!("/apps/{app_id}/secrets"),
            &query,
            None,
        )
        .await
    }

    pub async fn sync_secrets(&self, app_id: &str, body: Value) -> Result<Value, ClientError> {
        self.send_json(
            Method::POST,
            &format!("/apps/{app_id}/secrets/sync"),
            &[],
            Some(body),
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn list_logs(
        &self,
        app_id: &str,
        target: Option<&str>,
        since: Option<&str>,
        source: Option<&str>,
        level: Option<&str>,
        page_token: Option<&str>,
        limit: Option<u32>,
    ) -> Result<Value, ClientError> {
        let mut query: Vec<(&str, String)> = Vec::new();
        if let Some(v) = target {
            query.push(("target", v.to_owned()));
        }
        if let Some(v) = since {
            query.push(("since", v.to_owned()));
        }
        if let Some(v) = source {
            query.push(("source", v.to_owned()));
        }
        if let Some(v) = level {
            query.push(("level", v.to_owned()));
        }
        if let Some(t) = page_token {
            query.push(("pageToken", t.to_owned()));
        }
        if let Some(l) = limit {
            query.push(("limit", l.to_string()));
        }
        self.send_json(Method::GET, &format!("/apps/{app_id}/logs"), &query, None)
            .await
    }

    pub async fn get_runtime(&self, app_id: &str) -> Result<Value, ClientError> {
        self.send_json(Method::GET, &format!("/apps/{app_id}/runtime"), &[], None)
            .await
    }

    pub async fn list_domains(
        &self,
        app_id: &str,
        page_token: Option<&str>,
        limit: Option<u32>,
    ) -> Result<Value, ClientError> {
        let mut query: Vec<(&str, String)> = Vec::new();
        if let Some(t) = page_token {
            query.push(("pageToken", t.to_owned()));
        }
        if let Some(l) = limit {
            query.push(("limit", l.to_string()));
        }
        self.send_json(
            Method::GET,
            &format!("/apps/{app_id}/domains"),
            &query,
            None,
        )
        .await
    }

    pub async fn get_domain(&self, app_id: &str, domain_id: &str) -> Result<Value, ClientError> {
        self.send_json(
            Method::GET,
            &format!("/apps/{app_id}/domains/{domain_id}"),
            &[],
            None,
        )
        .await
    }

    pub async fn attach_domain(&self, app_id: &str, hostname: &str) -> Result<Value, ClientError> {
        self.send_json(
            Method::POST,
            &format!("/apps/{app_id}/domains"),
            &[],
            Some(json!({ "hostname": hostname })),
        )
        .await
    }

    pub async fn detach_domain(&self, app_id: &str, domain_id: &str) -> Result<Value, ClientError> {
        self.send_json(
            Method::DELETE,
            &format!("/apps/{app_id}/domains/{domain_id}"),
            &[],
            None,
        )
        .await
    }

    pub async fn list_subscriptions(
        &self,
        page_token: Option<&str>,
        limit: Option<u32>,
    ) -> Result<Value, ClientError> {
        let mut query: Vec<(&str, String)> = Vec::new();
        if let Some(t) = page_token {
            query.push(("pageToken", t.to_owned()));
        }
        if let Some(l) = limit {
            query.push(("limit", l.to_string()));
        }
        self.send_json(Method::GET, "/subscriptions", &query, None)
            .await
    }

    pub async fn get_app_subscription(&self, app_id: &str) -> Result<Value, ClientError> {
        self.send_json(
            Method::GET,
            &format!("/apps/{app_id}/subscription"),
            &[],
            None,
        )
        .await
    }

    pub async fn attach_subscription(
        &self,
        app_id: &str,
        subscription_id: &str,
    ) -> Result<Value, ClientError> {
        self.send_json(
            Method::PUT,
            &format!("/apps/{app_id}/subscription"),
            &[],
            Some(json!({ "subscriptionId": subscription_id })),
        )
        .await
    }
}

#[cfg(test)]
#[path = "client_tests.rs"]
mod tests;
