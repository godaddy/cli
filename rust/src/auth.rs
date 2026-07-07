use async_trait::async_trait;
use cli_engine::{
    CliCoreError, Credential, CredentialRequest, Result,
    auth::{AuthProvider, pkce::PkceAuthProvider},
};

use crate::environments::{self, ResolvedEnv};
use crate::pat::{self, PatEntry};

/// Single auth provider that dispatches to env-specific PKCE providers.
///
/// Each env's provider is named after the env, so cli-engine's
/// `PkceAuthProvider` picks up its per-env overrides automatically:
///   `<PREFIX>_OAUTH_CLIENT_ID`, `<PREFIX>_OAUTH_AUTH_URL`, `<PREFIX>_OAUTH_TOKEN_URL`
/// where `<PREFIX>` is the env name uppercased with `-` replaced by `_`
/// (e.g. `OTE_OAUTH_CLIENT_ID`, `DEV_OAUTH_AUTH_URL`). The API base URL and the
/// per-env defaults come from [`crate::environments`], which also resolves
/// custom DEV/TEST environments from the local config file (see
/// `crate::environments::environments_path`).
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
    log_resolved_oauth(env);
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

/// Build a `Credential` from a PAT entry.
///
/// The PAT is opaque to the CLI; the gateway exchanges it for an OAuth access
/// token. `identity` is set to a display name since the token itself carries no
/// readable subject claim.
fn pat_credential(env: &str, entry: &PatEntry) -> Credential {
    Credential {
        token: entry.token.clone(),
        provider: pat::PROVIDER.to_owned(),
        env: env.to_owned(),
        identity: if entry.name.is_empty() {
            "PAT".to_owned()
        } else {
            format!("PAT ({})", entry.name)
        },
        ..Credential::default()
    }
}

/// Emit (at debug level) the OAuth parameters that will be used for login and
/// the code→token exchange. cli-engine builds the actual token request, so this
/// is the CLI's single point of visibility into the client id / endpoints that
/// drive an `invalid_client`/`invalid_grant` failure.
///
/// It mirrors cli-engine's `<ENV>_OAUTH_*` env-var overrides
/// (`PkceAuthProvider::effective_*`) so the logged values are what's actually
/// sent — and flags when a value comes from an env var rather than config, which
/// is the usual cause of a "wrong client id". No secrets are logged (the OAuth
/// client id is a public identifier; tokens never pass through here).
///
/// Enable with `RUST_LOG=gddy=debug` (e.g. `RUST_LOG=gddy=debug gddy domain
/// available example.com --env dev`).
fn log_resolved_oauth(env: &ResolvedEnv) {
    if !tracing::enabled!(tracing::Level::DEBUG) {
        return;
    }
    let prefix = environments::env_prefix(&env.name);
    let override_var = |suffix: &str| std::env::var(format!("{prefix}_OAUTH_{suffix}")).ok();
    let client_id_ovr = override_var("CLIENT_ID");
    let auth_url_ovr = override_var("AUTH_URL");
    let token_url_ovr = override_var("TOKEN_URL");
    tracing::debug!(
        env = %env.name,
        client_id = %client_id_ovr.as_deref().unwrap_or(&env.client_id),
        client_id_from_env_var = client_id_ovr.is_some(),
        auth_url = %auth_url_ovr.as_deref().unwrap_or(&env.auth_url),
        auth_url_from_env_var = auth_url_ovr.is_some(),
        token_url = %token_url_ovr.as_deref().unwrap_or(&env.token_url),
        token_url_from_env_var = token_url_ovr.is_some(),
        redirect_uri = environments::REDIRECT_URI,
        "resolved OAuth client for login/token exchange"
    );
}

#[async_trait]
impl AuthProvider for GoDaddyAuthProvider {
    fn name(&self) -> &str {
        "godaddy"
    }

    async fn get_credential(&self, env: &str, command: &str, tier: &str) -> Result<Credential> {
        if let Some(entry) = pat::resolve_pat(env).await {
            return Ok(pat_credential(env, &entry));
        }
        let provider = self.provider_for(env)?;
        provider.get_credential(env, command, tier).await
    }

    async fn get_credential_for(&self, req: &CredentialRequest<'_>) -> Result<Credential> {
        // PATs have fixed scopes; the gateway enforces them. Use a PAT when one is
        // configured, otherwise fall back to PKCE OAuth scope step-up.
        if let Some(entry) = pat::resolve_pat(req.env).await {
            return Ok(pat_credential(req.env, &entry));
        }
        let provider = self.provider_for(req.env)?;
        provider.get_credential_for(req).await
    }

    async fn status(&self, env: &str) -> Result<Credential> {
        if let Some(entry) = pat::resolve_pat(env).await {
            return Ok(pat_credential(env, &entry));
        }
        let provider = self.provider_for(env)?;
        provider.status(env).await
    }

    async fn logout(&self, env: &str) -> Result<()> {
        // Remove any stored PAT for the environment; ignore errors because the
        // OAuth logout that follows is authoritative and PAT may not exist.
        if let Err(err) = pat::delete_pat(env).await {
            tracing::debug!(env, error = %err, "ignoring PAT delete error during logout");
        }
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
