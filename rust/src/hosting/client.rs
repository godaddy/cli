use hosting_client::types::{
    AppType, AttachDomainRequest, AttachSubscriptionRequest, CreateAppRequest, Environment,
    HostingProduct, ImportGitHubSourceRequest, RestartRequest,
};
use reqwest::{Client, Method};
use serde_json::{Value, json};

use crate::http::make_http_client;

const USER_AGENT: &str = concat!("godaddy-cli/", env!("CARGO_PKG_VERSION"));
const BASE_PATH: &str = "/v1/hosting";

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("HTTP error {status}: {body}")]
    Http { status: u16, body: String },
    #[error("network error: {0}")]
    Network(String),
    #[error("request error: {0}")]
    Request(String),
    #[error("failed to decode Hosting API response: {0}")]
    Response(#[from] serde_json::Error),
    #[error("failed to construct Hosting API client: {0}")]
    Build(#[from] hosting_client::BuildError),
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
            ClientError::Network(error) | ClientError::Request(error) => {
                Self::network(error).with_system("hosting")
            }
            ClientError::Response(error) => {
                Self::unexpected(format!("failed to decode Hosting API response: {error}"))
                    .with_system("hosting")
            }
            ClientError::Build(error) => {
                Self::config(format!("failed to construct Hosting API client: {error}"))
                    .with_system("hosting")
            }
            ClientError::Io { path, source } => {
                Self::validation(format!("failed to read {path}: {source}")).with_system("hosting")
            }
        }
    }
}

pub struct HostingClient {
    http: Client,
    base_url: String,
    token: String,
}

impl HostingClient {
    pub fn new(base_url: impl Into<String>, token: impl Into<String>) -> Self {
        Self {
            http: make_http_client(),
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

    /// Marker written to the `--debug transport` trace in place of a response
    /// body that carries a secret, so a minted token can never reach the debug
    /// output. The request breadcrumb and the response status/headers still log.
    const REDACTED_RESPONSE_BODY: &'static [u8] =
        b"<redacted: response body withheld (contains a credential)>";

    fn api(&self) -> Result<hosting_client::Client, ClientError> {
        hosting_client::client_with_auth(
            &self.base_url,
            &format!("Bearer {}", self.token),
            USER_AGENT,
            &Self::new_request_id(),
        )
        .map_err(ClientError::Build)
    }

    // JSON Patch (RFC 6902) requires application/json-patch+json, which the
    // generated client does not set. Keep PATCH app/secrets on this path.
    async fn send_patch(
        &self,
        path: &str,
        query: &[(&str, String)],
        body: Value,
    ) -> Result<Value, ClientError> {
        let body_str = serde_json::to_string(&body)?;

        let mut req = self
            .http
            .request(Method::PATCH, self.url(path))
            .bearer_auth(&self.token)
            .header("x-request-id", Self::new_request_id())
            .header("content-type", "application/json-patch+json")
            .body(body_str);

        for (key, value) in query {
            req = req.query(&[(key, value)]);
        }

        let request = req
            .build()
            .map_err(|e| ClientError::Network(e.to_string()))?;
        cli_engine::transport::debug_log_reqwest_request(&request);
        let resp = self
            .http
            .execute(request)
            .await
            .map_err(|e| ClientError::Network(e.to_string()))?;

        let status = resp.status();
        let headers = resp.headers().clone();
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| ClientError::Network(e.to_string()))?;
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

        serde_json::from_slice(&bytes).map_err(ClientError::Response)
    }

    // Spec has no request body; Akamai still 411s a POST with no Content-Length.
    async fn post_empty_json(&self, path: &str) -> Result<Value, ClientError> {
        self.post_empty_json_inner(path, true).await
    }

    /// Like [`post_empty_json`](Self::post_empty_json), but the response body is
    /// withheld from the `--debug transport` trace. Use for endpoints whose
    /// response carries a secret — e.g. the agent-token mint, whose body is a
    /// bearer token. cli-engine redacts sensitive *headers* but prints bodies
    /// verbatim and offers no body-redaction hook, so the suppression happens
    /// here, at the one call site that needs it.
    async fn post_empty_json_secret_response(&self, path: &str) -> Result<Value, ClientError> {
        self.post_empty_json_inner(path, false).await
    }

    async fn post_empty_json_inner(
        &self,
        path: &str,
        log_response_body: bool,
    ) -> Result<Value, ClientError> {
        let request = self
            .http
            .request(Method::POST, self.url(path))
            .bearer_auth(&self.token)
            .header("x-request-id", Self::new_request_id())
            .json(&json!({}))
            .build()
            .map_err(|e| ClientError::Network(e.to_string()))?;
        cli_engine::transport::debug_log_reqwest_request(&request);
        let resp = self
            .http
            .execute(request)
            .await
            .map_err(|e| ClientError::Network(e.to_string()))?;

        let status = resp.status();
        let headers = resp.headers().clone();
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| ClientError::Network(e.to_string()))?;
        // Under `--debug transport` cli-engine prints response bodies verbatim
        // (only sensitive headers are redacted). For a secret-bearing response
        // hand the logger a fixed marker instead of the real bytes, so a minted
        // token cannot leak into the trace; status and headers still log.
        cli_engine::transport::debug_log_reqwest_response(
            status,
            &headers,
            if log_response_body {
                bytes.as_ref()
            } else {
                Self::REDACTED_RESPONSE_BODY
            },
        );

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
        serde_json::from_slice(&bytes).map_err(ClientError::Response)
    }

    pub async fn list_apps(
        &self,
        app_type: &str,
        page_token: Option<&str>,
        limit: Option<u32>,
    ) -> Result<Value, ClientError> {
        let client = self.api()?;
        let mut request = client
            .list_apps()
            .app_type(AppType::from(app_type.to_owned()));
        if let Some(token) = page_token {
            request = request.page_token(token.to_owned());
        }
        if let Some(limit) = limit
            && let Some(size) = page_size(limit)
        {
            request = request.page_size(size);
        }
        response(request.send().await).await
    }

    pub async fn get_app(&self, app_id: &str) -> Result<Value, ClientError> {
        let client = self.api()?;
        response(client.get_app().app_id(app_id).send().await).await
    }

    pub async fn create_app(&self, app_type: &str, body: Value) -> Result<Value, ClientError> {
        let client = self.api()?;
        let body: CreateAppRequest = deserialize(body)?;
        response(
            client
                .create_app()
                .app_type(AppType::from(app_type.to_owned()))
                .body(body)
                .send()
                .await,
        )
        .await
    }

    pub async fn update_app(&self, app_id: &str, patch: Value) -> Result<Value, ClientError> {
        self.send_patch(&format!("/apps/{app_id}"), &[], patch)
            .await
    }

    pub async fn delete_app(&self, app_id: &str) -> Result<Value, ClientError> {
        let client = self.api()?;
        response(client.delete_app().app_id(app_id).send().await).await
    }

    pub async fn get_app_status(&self, app_id: &str) -> Result<Value, ClientError> {
        let client = self.api()?;
        response(client.get_app_status().app_id(app_id).send().await).await
    }

    pub async fn restart_app(&self, app_id: &str, variant: &str) -> Result<Value, ClientError> {
        let client = self.api()?;
        let body: RestartRequest = deserialize(json!({
            "variant": variant,
        }))?;
        response(
            client
                .create_app_restart()
                .app_id(app_id)
                .body(body)
                .send()
                .await,
        )
        .await
    }

    /// Mint a short-lived agent token for the app and return the agent's
    /// assigned URL alongside it. Response shape: `{ agentUrl, token, expires? }`.
    /// The `db tunnel` caller mints this token with a dedicated
    /// `hosting.database.tunnel:execute` scope in addition to `hosting.deployment:execute`,
    /// so publish authority alone does not yield a database-tunnel agent token.
    ///
    /// This mint lives under the Node.js-specific `/v1/hosting/nodejs` path
    /// rather than the flattened `/v1/hosting` base the other methods use, so it
    /// is spelled out with a leading `/nodejs` segment to reach the server route.
    pub async fn get_agent_token(&self, app_id: &str) -> Result<Value, ClientError> {
        // Secret response: the body is a minted bearer token, so it must never
        // reach the `--debug transport` trace (cli-engine would print it in full).
        self.post_empty_json_secret_response(&format!("/nodejs/apps/{app_id}/agent-token"))
            .await
    }

    pub async fn list_deployments(
        &self,
        app_id: &str,
        page_token: Option<&str>,
        limit: Option<u32>,
    ) -> Result<Value, ClientError> {
        let client = self.api()?;
        let mut request = client.list_deployments().app_id(app_id);
        if let Some(token) = page_token {
            request = request.page_token(token.to_owned());
        }
        if let Some(limit) = limit
            && let Some(size) = page_size(limit)
        {
            request = request.page_size(size);
        }
        response(request.send().await).await
    }

    pub async fn get_deployment(
        &self,
        app_id: &str,
        deployment_id: &str,
    ) -> Result<Value, ClientError> {
        let client = self.api()?;
        response(
            client
                .get_deployment()
                .app_id(app_id)
                .deployment_id(deployment_id)
                .send()
                .await,
        )
        .await
    }

    pub async fn create_deployment(&self, app_id: &str) -> Result<Value, ClientError> {
        self.post_empty_json(&format!("/apps/{app_id}/deployments"))
            .await
    }

    pub async fn get_operation(&self, operation_id: &str) -> Result<Value, ClientError> {
        let client = self.api()?;
        response(
            client
                .get_app_operation()
                .operation_id(operation_id)
                .send()
                .await,
        )
        .await
    }

    pub async fn create_import(
        &self,
        app_id: &str,
        repo: &str,
        branch: &str,
    ) -> Result<Value, ClientError> {
        let client = self.api()?;
        let body = ImportGitHubSourceRequest {
            repository_full_name: Some(repo.to_owned()),
            branch: Some(branch.to_owned()),
        };
        response(
            client
                .create_source_import()
                .app_id(app_id)
                .body(body)
                .send()
                .await,
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
            .http
            .post(self.url(&format!("/apps/{app_id}/imports")))
            .bearer_auth(&self.token)
            .header("x-request-id", Self::new_request_id())
            .multipart(form)
            .build()
            .map_err(|e| ClientError::Network(e.to_string()))?;
        cli_engine::transport::debug_log_reqwest_request(&request);
        let resp = self
            .http
            .execute(request)
            .await
            .map_err(|e| ClientError::Network(e.to_string()))?;

        let status = resp.status();
        let headers = resp.headers().clone();
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| ClientError::Network(e.to_string()))?;
        cli_engine::transport::debug_log_reqwest_response(status, &headers, &bytes);

        let status = status.as_u16();
        if !(200..300).contains(&status) {
            return Err(ClientError::Http {
                status,
                body: String::from_utf8_lossy(&bytes).into_owned(),
            });
        }

        serde_json::from_slice(&bytes).map_err(ClientError::Response)
    }

    pub async fn get_import(&self, app_id: &str, import_id: &str) -> Result<Value, ClientError> {
        let client = self.api()?;
        response(
            client
                .get_source_import()
                .app_id(app_id)
                .import_id(import_id)
                .send()
                .await,
        )
        .await
    }

    pub async fn list_secrets(
        &self,
        app_id: &str,
        variant: Option<&str>,
    ) -> Result<Value, ClientError> {
        let client = self.api()?;
        let mut request = client.list_secrets().app_id(app_id);
        if let Some(variant) = variant {
            request = request.variant(Environment::from(variant.to_owned()));
        }
        response(request.send().await).await
    }

    pub async fn patch_secrets(
        &self,
        app_id: &str,
        variant: &str,
        patch: Value,
    ) -> Result<Value, ClientError> {
        self.send_patch(
            &format!("/apps/{app_id}/secrets"),
            &[("variant", variant.to_owned())],
            patch,
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
        let client = self.api()?;
        let mut request = client.get_logs().app_id(app_id);
        if let Some(target) = target {
            request = request.target(target.to_owned());
        }
        if let Some(since) = since {
            request = request.since(since.to_owned());
        }
        if let Some(source) = source {
            request = request.source(source.to_owned());
        }
        if let Some(level) = level {
            request = request.level(level.to_owned());
        }
        if let Some(token) = page_token {
            request = request.page_token(token.to_owned());
        }
        if let Some(limit) = limit
            && let Some(size) = page_size(limit)
        {
            request = request.page_size(size);
        }
        response(request.send().await).await
    }

    pub async fn get_runtime(&self, app_id: &str) -> Result<Value, ClientError> {
        let client = self.api()?;
        response(client.get_runtime().app_id(app_id).send().await).await
    }

    pub async fn list_domains(
        &self,
        app_id: &str,
        page_token: Option<&str>,
        limit: Option<u32>,
    ) -> Result<Value, ClientError> {
        let client = self.api()?;
        let request = client.list_domains().app_id(app_id);
        let _ = (page_token, limit);
        response(request.send().await).await
    }

    pub async fn get_domain(&self, app_id: &str, domain_id: &str) -> Result<Value, ClientError> {
        let client = self.api()?;
        response(
            client
                .get_domain()
                .app_id(app_id)
                .domain_id(domain_id)
                .send()
                .await,
        )
        .await
    }

    pub async fn attach_domain(&self, app_id: &str, hostname: &str) -> Result<Value, ClientError> {
        let client = self.api()?;
        let body: AttachDomainRequest = deserialize(json!({
            "hostname": hostname,
        }))?;
        response(
            client
                .attach_domain()
                .app_id(app_id)
                .body(body)
                .send()
                .await,
        )
        .await
    }

    pub async fn detach_domain(&self, app_id: &str, domain_id: &str) -> Result<Value, ClientError> {
        let client = self.api()?;
        response(
            client
                .detach_domain()
                .app_id(app_id)
                .domain_id(domain_id)
                .send()
                .await,
        )
        .await
    }

    pub async fn list_subscriptions(
        &self,
        page_token: Option<&str>,
        limit: Option<u32>,
        hosting_product: &str,
    ) -> Result<Value, ClientError> {
        let client = self.api()?;
        let request = client
            .list_subscriptions()
            .hosting_product(HostingProduct::from(hosting_product.to_owned()));
        let _ = (page_token, limit);
        response(request.send().await).await
    }

    pub async fn get_app_subscription(&self, app_id: &str) -> Result<Value, ClientError> {
        let client = self.api()?;
        response(
            client
                .get_subscription_attachment()
                .app_id(app_id)
                .send()
                .await,
        )
        .await
    }

    pub async fn attach_subscription(
        &self,
        app_id: &str,
        subscription_id: &str,
    ) -> Result<Value, ClientError> {
        let client = self.api()?;
        let body: AttachSubscriptionRequest = deserialize(json!({
            "subscriptionId": subscription_id,
        }))?;
        response(
            client
                .attach_subscription()
                .app_id(app_id)
                .body(body)
                .send()
                .await,
        )
        .await
    }
}

fn page_size(limit: u32) -> Option<std::num::NonZeroU64> {
    std::num::NonZeroU64::new(u64::from(limit))
}

fn deserialize<T: serde::de::DeserializeOwned>(body: Value) -> Result<T, ClientError> {
    serde_json::from_value(body).map_err(ClientError::Response)
}

async fn response<T: serde::Serialize>(
    response: Result<progenitor_client::ResponseValue<T>, progenitor_client::Error<()>>,
) -> Result<Value, ClientError> {
    match response {
        Ok(response) => serde_json::to_value(response.into_inner()).map_err(ClientError::Response),
        Err(progenitor_client::Error::InvalidResponsePayload(bytes, _)) if bytes.is_empty() => {
            Ok(Value::Null)
        }
        Err(progenitor_client::Error::InvalidResponsePayload(bytes, error)) => {
            Err(ClientError::Request(format!(
                "failed to decode Hosting API response: {error} (body: {})",
                String::from_utf8_lossy(&bytes)
            )))
        }
        Err(progenitor_client::Error::UnexpectedResponse(response))
            if response.status().is_success() =>
        {
            let bytes = response.bytes().await.unwrap_or_default();
            if bytes.is_empty() {
                Ok(Value::Null)
            } else {
                serde_json::from_slice(&bytes).map_err(ClientError::Response)
            }
        }
        Err(progenitor_client::Error::UnexpectedResponse(response)) => {
            let status = response.status().as_u16();
            let body = response.text().await.unwrap_or_default();
            Err(ClientError::Http { status, body })
        }
        Err(progenitor_client::Error::CommunicationError(error)) => {
            Err(ClientError::Network(error.to_string()))
        }
        Err(error) => Err(ClientError::Request(error.to_string())),
    }
}

#[cfg(test)]
#[path = "client_tests.rs"]
mod tests;
