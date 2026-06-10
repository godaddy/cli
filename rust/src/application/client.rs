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
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.token)
            .header("x-request-id", new_request_id())
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let text = resp.text().await.unwrap_or_default();
            return Err(ClientError::Http { status, body: text });
        }

        let payload: Value = resp.json().await?;
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
            "query": "mutation CreateRelease($input: MutationCreateReleaseInput!) { createRelease(input: $input) { id version description createdAt } }",
            "variables": { "input": input }
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

    pub async fn upload_artifact(
        &self,
        url: &str,
        headers: &Value,
        bytes: Vec<u8>,
    ) -> Result<(), ClientError> {
        let mut req = self.client.put(url).body(bytes);
        if let Some(obj) = headers.as_object() {
            for (k, v) in obj {
                if let Some(s) = v.as_str() {
                    req = req.header(k.as_str(), s);
                }
            }
        }
        let resp = req.send().await?;
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(ClientError::Http { status, body });
        }
        Ok(())
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
    use super::api_url_for_env;

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
        assert!(url.starts_with("https://"));
    }
}
