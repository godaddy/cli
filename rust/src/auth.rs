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
    let prefix = env.name.to_uppercase().replace('-', "_");
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

/// Stored in [`Credential::provider`] for the sso-key bypass path, so the domain
/// client selects the `sso-key` Authorization scheme instead of Bearer.
pub const SSO_KEY_PROVIDER: &str = "sso-key";

/// Process-global sso-key supplied via the `--api-key`/`--api-secret` flags.
///
/// The flags are bridged here (rather than to `<ENV>_API_KEY` env vars, which
/// edition-2024 makes `unsafe` to set) by [`set_api_key_override`] during flag
/// application; the composite provider consults it before the per-env config.
static API_KEY_OVERRIDE: std::sync::OnceLock<std::sync::Mutex<Option<(String, String)>>> =
    std::sync::OnceLock::new();

fn api_key_override_cell() -> &'static std::sync::Mutex<Option<(String, String)>> {
    API_KEY_OVERRIDE.get_or_init(|| std::sync::Mutex::new(None))
}

/// Record an sso-key from the `--api-key`/`--api-secret` flags. Highest
/// precedence for domain-command auth (beats `<ENV>_API_KEY` and config).
pub fn set_api_key_override(key: String, secret: String) {
    if let Ok(mut guard) = api_key_override_cell().lock() {
        *guard = Some((key, secret));
    }
}

fn api_key_override() -> Option<(String, String)> {
    api_key_override_cell().lock().ok().and_then(|g| g.clone())
}

/// Auth provider that composes [`GoDaddyAuthProvider`] (OAuth/PKCE) but, for
/// `domain:*` commands whose target environment has an sso-key configured,
/// returns that key instead.
///
/// The GoDaddy Domains API endpoints authenticate with
/// `Authorization: sso-key <KEY>:<SECRET>`, not OAuth. Every other command — and
/// any domain command in an environment without a configured key — continues to
/// use OAuth (including scope step-up). Scoping the bypass to `domain:*` keeps it
/// from affecting unrelated commands.
#[derive(Debug, Default)]
pub struct CompositeAuthProvider {
    oauth: GoDaddyAuthProvider,
}

impl CompositeAuthProvider {
    pub fn new() -> Self {
        Self {
            oauth: GoDaddyAuthProvider::new(),
        }
    }

    /// Build an sso-key credential, if this is a `domain:*` command and both a
    /// key and secret are present. Pure (no process/config access) for testing.
    fn sso_key_credential_from(
        env: &str,
        command: &str,
        key: Option<&str>,
        secret: Option<&str>,
    ) -> Option<Credential> {
        if !command.starts_with("domain:") {
            return None;
        }
        let key = key.map(str::trim).filter(|s| !s.is_empty())?;
        let secret = secret.map(str::trim).filter(|s| !s.is_empty())?;
        Some(Credential {
            token: format!("{key}:{secret}"),
            provider: SSO_KEY_PROVIDER.to_owned(),
            env: env.to_owned(),
            ..Default::default()
        })
    }

    /// Resolve the sso-key for a domain command (flag override → per-env config)
    /// and turn it into a credential.
    fn sso_key_credential(env: &str, command: &str) -> Option<Credential> {
        if let Some((key, secret)) = api_key_override() {
            return Self::sso_key_credential_from(env, command, Some(&key), Some(&secret));
        }
        let domains = environments::resolve_domains(env).ok()?;
        Self::sso_key_credential_from(
            env,
            command,
            domains.api_key.as_deref(),
            domains.api_secret.as_deref(),
        )
    }
}

#[async_trait]
impl AuthProvider for CompositeAuthProvider {
    fn name(&self) -> &str {
        self.oauth.name()
    }

    async fn get_credential(&self, env: &str, command: &str, tier: &str) -> Result<Credential> {
        if let Some(cred) = Self::sso_key_credential(env, command) {
            return Ok(cred);
        }
        self.oauth.get_credential(env, command, tier).await
    }

    async fn get_credential_for(&self, req: &CredentialRequest<'_>) -> Result<Credential> {
        if let Some(cred) = Self::sso_key_credential(req.env, req.command) {
            return Ok(cred);
        }
        self.oauth.get_credential_for(req).await
    }

    async fn status(&self, env: &str) -> Result<Credential> {
        self.oauth.status(env).await
    }

    async fn logout(&self, env: &str) -> Result<()> {
        self.oauth.logout(env).await
    }

    async fn list_environments(&self) -> Result<Vec<String>> {
        self.oauth.list_environments().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sso_key_only_for_domain_commands_with_key_and_secret() {
        // domain command + both key/secret -> sso-key credential.
        let cred = CompositeAuthProvider::sso_key_credential_from(
            "ote",
            "domain:available",
            Some("KEY"),
            Some("SECRET"),
        )
        .expect("sso-key credential");
        assert_eq!(cred.token, "KEY:SECRET");
        assert_eq!(cred.provider, SSO_KEY_PROVIDER);
        assert_eq!(cred.env, "ote");
    }

    #[test]
    fn no_sso_key_for_non_domain_commands() {
        assert!(
            CompositeAuthProvider::sso_key_credential_from(
                "ote",
                "application:list",
                Some("KEY"),
                Some("SECRET"),
            )
            .is_none()
        );
    }

    #[test]
    fn no_sso_key_when_key_or_secret_missing_or_blank() {
        assert!(
            CompositeAuthProvider::sso_key_credential_from(
                "ote",
                "domain:suggest",
                Some("KEY"),
                None
            )
            .is_none()
        );
        assert!(
            CompositeAuthProvider::sso_key_credential_from(
                "ote",
                "domain:suggest",
                Some("  "),
                Some("SECRET"),
            )
            .is_none()
        );
    }
}
