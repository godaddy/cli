use std::sync::Arc;

use async_trait::async_trait;
use cli_engine::{
    CliCoreError, Credential, CredentialRequest, Result,
    auth::{AuthProvider, pkce::PkceAuthProvider},
};

use crate::environments::{self, ResolvedEnv};
use crate::pat::{self, PatEntry};
use crate::scopes;

#[derive(Debug)]
pub struct GoDaddyAuthProvider {
    provider: PkceAuthProvider,
}

impl GoDaddyAuthProvider {
    pub fn new() -> Self {
        Self {
            provider: PkceAuthProvider::new(
                "godaddy",
                "",
                "",
                "",
                environments::DEFAULT_OAUTH_SCOPES,
            )
            .with_app_id(environments::APP_ID)
            .with_redirect_uri(environments::REDIRECT_URI)
            .with_environments(Arc::clone(environments::instance())),
        }
    }

    /// Resolves `env` and logs the OAuth parameters that will be used
    /// (see [`log_resolved_oauth`]).
    ///
    /// Resolves up front (rather than relying solely on `.with_environments`'s
    /// internal, silently-degrading resolution) so an unknown env surfaces a
    /// clear error here instead of a confusing OAuth failure later.
    fn resolve_and_log(&self, env: &str) -> Result<()> {
        let resolved = environments::resolve(env)?;
        log_resolved_oauth(&resolved);
        Ok(())
    }
}

impl Default for GoDaddyAuthProvider {
    fn default() -> Self {
        Self::new()
    }
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

/// Rejects any scope in `requested` that isn't registered in [`scopes`] —
/// [`scopes::ALL`] plus the standalone [`scopes::OFFLINE_ACCESS`] directive
/// scope.
///
/// A scope missing from that list is, by construction, either misspelled or
/// was never registered on the CLI's OAuth client (see the "Adding a scope"
/// steps in the [`scopes`] module docs), so the authorization server would
/// reject it anyway. Catching it here turns that into a clear, local error
/// instead of the auth server's `invalid_scope`.
fn validate_requested_scopes(requested: &[String]) -> Result<()> {
    let mut seen = std::collections::HashSet::new();
    let unknown: Vec<&str> = requested
        .iter()
        .map(String::as_str)
        .filter(|scope| *scope != scopes::OFFLINE_ACCESS && !scopes::ALL.contains(scope))
        .filter(|scope| seen.insert(*scope))
        .collect();
    if unknown.is_empty() {
        return Ok(());
    }
    Err(CliCoreError::message(format!(
        "unsupported OAuth scope(s) requested: {}",
        unknown.join(", ")
    )))
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
        self.resolve_and_log(env)?;
        self.provider.get_credential(env, command, tier).await
    }

    async fn get_credential_for(&self, req: &CredentialRequest<'_>) -> Result<Credential> {
        // PATs have fixed scopes; the gateway enforces them. Use a PAT when one is
        // configured, otherwise fall back to PKCE OAuth scope step-up.
        if let Some(entry) = pat::resolve_pat(req.env).await {
            return Ok(pat_credential(req.env, &entry));
        }
        // `Dispatcher::login_with_scopes` (the handler behind `auth login
        // --scope`) synthesizes its `CredentialRequest` with an empty command
        // path — no ordinary command routes through `get_credential_for` with
        // `command` unset. That makes it the one place to validate explicitly
        // *requested* scopes against the CLI's authoritative registry before
        // asking the authorization server, so a misspelled or unregistered
        // `--scope` fails fast with a readable message instead of surfacing as
        // the auth server's opaque `invalid_scope`. Regular commands'
        // `with_scopes`-declared scopes, and `api call --scope`'s ad-hoc
        // catalog-derived scopes, are deliberately not re-validated here — see
        // the module docs on [`scopes`] for why those fall outside the
        // registry.
        if req.command.is_empty() {
            validate_requested_scopes(&req.meta.scopes)?;
        }
        self.resolve_and_log(req.env)?;
        self.provider.get_credential_for(req).await
    }

    async fn status(&self, env: &str) -> Result<Credential> {
        if let Some(entry) = pat::resolve_pat(env).await {
            return Ok(pat_credential(env, &entry));
        }
        self.resolve_and_log(env)?;
        self.provider.status(env).await
    }

    async fn logout(&self, env: &str) -> Result<()> {
        // Remove any stored PAT for the environment; ignore errors because the
        // OAuth logout that follows is authoritative and PAT may not exist.
        if let Err(err) = pat::delete_pat(env).await {
            tracing::debug!(env, error = %err, "ignoring PAT delete error during logout");
        }
        self.resolve_and_log(env)?;
        self.provider.logout(env).await
    }

    async fn list_environments(&self) -> Result<Vec<String>> {
        // `PkceAuthProvider::list_environments` only reflects its own
        // in-memory token cache (keyring/file storage can't be enumerated by
        // prefix), so this is a single call now that credentials for every
        // env share one provider instance — no need to loop per env
        // constructing a fresh (always cache-empty) provider per iteration.
        let mut envs = std::collections::BTreeSet::new();
        match pat::registry_envs().await {
            Ok(pats) => envs.extend(pats),
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    "failed to load PAT registry while listing environments; continuing"
                );
            }
        }
        envs.extend(self.provider.list_environments().await.unwrap_or_default());
        Ok(envs.into_iter().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_requested_scopes_accepts_known_and_empty() {
        assert!(validate_requested_scopes(&[]).is_ok());
        assert!(validate_requested_scopes(&[scopes::DOMAINS_READ.to_owned()]).is_ok());
        assert!(
            validate_requested_scopes(&[
                scopes::DOMAINS_READ.to_owned(),
                scopes::HOSTING_APPS_READ.to_owned(),
            ])
            .is_ok()
        );
    }

    #[test]
    fn validate_requested_scopes_accepts_offline_access() {
        assert!(validate_requested_scopes(&[scopes::OFFLINE_ACCESS.to_owned()]).is_ok());
    }

    #[test]
    fn validate_requested_scopes_rejects_unregistered_scope() {
        let err = validate_requested_scopes(&["domains.domain:write".to_owned()])
            .expect_err("scope is not in the registry");
        assert_eq!(
            err.to_string(),
            "unsupported OAuth scope(s) requested: domains.domain:write"
        );
    }

    #[test]
    fn validate_requested_scopes_reports_every_unknown_scope() {
        let err = validate_requested_scopes(&[
            scopes::DOMAINS_READ.to_owned(),
            "bogus:scope".to_owned(),
            "another:bogus".to_owned(),
        ])
        .expect_err("two scopes are not in the registry");
        let message = err.to_string();
        assert!(message.contains("bogus:scope"));
        assert!(message.contains("another:bogus"));
        assert!(!message.contains(scopes::DOMAINS_READ));
    }

    #[test]
    fn validate_requested_scopes_dedupes_repeated_unknown_scopes() {
        let err = validate_requested_scopes(&["bogus:scope".to_owned(), "bogus:scope".to_owned()])
            .expect_err("scope is not in the registry");
        assert_eq!(
            err.to_string(),
            "unsupported OAuth scope(s) requested: bogus:scope"
        );
    }

    /// PAT scopes are opaque and enforced entirely server-side (see
    /// `src/pat/guides/auth.md`); the CLI must never fabricate a scopes list
    /// for a PAT-backed credential, or `gddy auth status`/an eager-login
    /// planner built on it would report scopes the CLI has no actual
    /// visibility into.
    #[test]
    fn pat_credential_has_empty_scopes() {
        let entry = PatEntry {
            token: "gdapikeyabc123".to_owned(),
            name: "work".to_owned(),
        };
        let credential = pat_credential("prod", &entry);
        assert!(credential.scopes.is_empty());
    }
}
