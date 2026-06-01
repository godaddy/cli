use async_trait::async_trait;
use cli_engine::{
    CliCoreError, Credential, Result,
    auth::{AuthProvider, pkce::PkceAuthProvider},
};

const OTE_API_URL: &str = "https://api.ote-godaddy.com";
const PROD_API_URL: &str = "https://api.godaddy.com";

const OTE_CLIENT_ID: &str = "a502484b-d7b1-4509-aa88-08b391a54c28";
const PROD_CLIENT_ID: &str = "39489dee-4103-4284-9aab-9f2452142bce";

const SCOPES: &[&str] = &["apps.app-registry:read", "apps.app-registry:write"];

/// Single auth provider that dispatches to env-specific PKCE providers.
///
/// Env var overrides (per-env):
///   OTE:  OTE_OAUTH_CLIENT_ID, OTE_OAUTH_AUTH_URL, OTE_OAUTH_TOKEN_URL
///   PROD: PROD_OAUTH_CLIENT_ID, PROD_OAUTH_AUTH_URL, PROD_OAUTH_TOKEN_URL
#[derive(Debug)]
pub struct GoDaddyAuthProvider {
    ote: PkceAuthProvider,
    prod: PkceAuthProvider,
}

impl GoDaddyAuthProvider {
    pub fn new() -> Self {
        let ote = PkceAuthProvider::new(
            "ote",
            format!("{OTE_API_URL}/v2/oauth2/authorize"),
            format!("{OTE_API_URL}/v2/oauth2/token"),
            OTE_CLIENT_ID,
            SCOPES,
        )
        .with_app_id("godaddy")
        .with_redirect_uri("http://localhost:7443/callback");

        let prod = PkceAuthProvider::new(
            "prod",
            format!("{PROD_API_URL}/v2/oauth2/authorize"),
            format!("{PROD_API_URL}/v2/oauth2/token"),
            PROD_CLIENT_ID,
            SCOPES,
        )
        .with_app_id("godaddy")
        .with_redirect_uri("http://localhost:7443/callback");

        Self { ote, prod }
    }

    fn provider_for(&self, env: &str) -> Result<&PkceAuthProvider> {
        match env {
            "ote" => Ok(&self.ote),
            "prod" => Ok(&self.prod),
            _ => Err(CliCoreError::message(format!(
                "unknown environment {env:?}; expected \"ote\" or \"prod\""
            ))),
        }
    }
}

#[async_trait]
impl AuthProvider for GoDaddyAuthProvider {
    fn name(&self) -> &str {
        "godaddy"
    }

    async fn get_credential(&self, env: &str, command: &str, tier: &str) -> Result<Credential> {
        self.provider_for(env)?.get_credential(env, command, tier).await
    }

    async fn status(&self, env: &str) -> Result<Credential> {
        self.provider_for(env)?.status(env).await
    }

    async fn logout(&self, env: &str) -> Result<()> {
        self.provider_for(env)?.logout(env).await
    }

    async fn list_environments(&self) -> Result<Vec<String>> {
        let mut envs = self.ote.list_environments().await.unwrap_or_default();
        envs.extend(self.prod.list_environments().await.unwrap_or_default());
        envs.dedup();
        Ok(envs)
    }
}
