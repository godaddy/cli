use async_trait::async_trait;
use cli_engine::{
    CliCoreError, Credential, CredentialRequest, Result,
    auth::{AuthProvider, pkce::PkceAuthProvider},
};

use crate::environments::{self, ResolvedEnv};

/// Single auth provider that dispatches to env-specific PKCE providers.
///
/// Each env's provider is named after the env, so cli-engine's
/// `PkceAuthProvider` picks up its per-env overrides automatically:
///   `<PREFIX>_OAUTH_CLIENT_ID`, `<PREFIX>_OAUTH_AUTH_URL`, `<PREFIX>_OAUTH_TOKEN_URL`
/// where `<PREFIX>` is the env name uppercased with `-` replaced by `_`
/// (e.g. `OTE_OAUTH_CLIENT_ID`, `DEV_OAUTH_AUTH_URL`). The API base URL and the
/// per-env defaults come from [`crate::environments`], which also resolves
/// custom DEV/TEST environments from `~/.config/gddy/environments.toml`.
#[derive(Debug, Default)]
pub struct GoDaddyAuthProvider;

impl GoDaddyAuthProvider {
    pub fn new() -> Self {
        Self
    }

    /// Build a PKCE provider for the given env by resolving its endpoints.
    ///
    /// Providers are constructed on demand (tokens persist in the OS keychain,
    /// so there is nothing to cache across a one-shot CLI invocation). Works for
    /// built-ins as well as any custom env defined via env var or local config.
    fn provider_for(&self, env: &str) -> Result<PkceAuthProvider> {
        let resolved =
            environments::resolve(env).map_err(|e| CliCoreError::message(e.to_string()))?;
        Ok(build_provider(&resolved))
    }
}

fn build_provider(env: &ResolvedEnv) -> PkceAuthProvider {
    PkceAuthProvider::new(
        env.name.clone(),
        env.auth_url.clone(),
        env.token_url.clone(),
        env.client_id.clone(),
        environments::DEFAULT_OAUTH_SCOPES,
    )
    .with_app_id(environments::APP_ID)
    .with_redirect_uri(environments::REDIRECT_URI)
}

#[async_trait]
impl AuthProvider for GoDaddyAuthProvider {
    fn name(&self) -> &str {
        "godaddy"
    }

    async fn get_credential(&self, env: &str, command: &str, tier: &str) -> Result<Credential> {
        let provider = self.provider_for(env)?;
        provider.get_credential(env, command, tier).await
    }

    async fn get_credential_for(&self, req: &CredentialRequest<'_>) -> Result<Credential> {
        // Forward to the env's PKCE provider, which performs OAuth scope step-up
        // when the cached token lacks the command's required scopes.
        let provider = self.provider_for(req.env)?;
        provider.get_credential_for(req).await
    }

    async fn status(&self, env: &str) -> Result<Credential> {
        let provider = self.provider_for(env)?;
        provider.status(env).await
    }

    async fn logout(&self, env: &str) -> Result<()> {
        let provider = self.provider_for(env)?;
        provider.logout(env).await
    }

    async fn list_environments(&self) -> Result<Vec<String>> {
        // Enumerate stored credentials across built-ins + locally-configured
        // envs (env-var-only envs are excluded from `listable`, matching the
        // `env list` contract). `listable` falls back to built-ins (logging a
        // warning) on a malformed local config, so this never fails wholesale.
        let listable =
            environments::listable().map_err(|e| CliCoreError::message(e.to_string()))?;
        let mut envs = Vec::new();
        for resolved in listable {
            let provider = build_provider(&resolved);
            envs.extend(provider.list_environments().await.unwrap_or_default());
        }
        envs.sort();
        envs.dedup();
        Ok(envs)
    }
}
