use std::sync::Arc;

use async_trait::async_trait;
use cli_engine::{
    CliCoreError, Credential, CredentialRequest, Result,
    auth::{AuthProvider, pkce::PkceAuthProvider},
};

use crate::environments::{self, GddyEnvConfig};
use crate::pat::{self, PatEntry};
use crate::scopes;

/// Single auth provider, built per call from gddy's own resolved
/// [`GddyEnvConfig`] (already fully derived — `auth_url`/`token_url` filled
/// in from `api_url` wherever the environment left them blank — by its own
/// `#[env_config(...)]` attributes, not by anything gddy-specific bolted on
/// after resolution), then wired via `.with_environments` so cli-engine's
/// own resolution stays consistent with gddy's.
///
/// Passing static empty base args to a single, long-lived `PkceAuthProvider`
/// (as this used to) would mean any environment relying on that derivation
/// (e.g. the real `dev`/`test` file entries, which only set `client_id`)
/// tries to hit an empty auth/token URL — a real, reproduced bug, not a
/// hypothetical one. So each call resolves `env` first and passes the
/// fully-derived values in as the provider's base args.
///
/// The provider is still always constructed with the fixed name `"godaddy"`
/// (not `env`), so cli-engine's credential storage key
/// (`app_id`/`provider_name`/`env`) stays `(app_id, "godaddy", env)`
/// regardless of how many times this rebuilds the provider — the one-time
/// credential-key change (and required re-login) from collapsing gddy's
/// former per-env-named providers happens exactly once, not per call.
#[derive(Debug, Default)]
pub struct GoDaddyAuthProvider;

impl GoDaddyAuthProvider {
    pub fn new() -> Self {
        Self
    }

    /// Resolves `env`, logs the OAuth parameters that will be used (see
    /// [`log_resolved_oauth`]), and builds a `PkceAuthProvider` from the
    /// fully-derived values.
    ///
    /// Resolves up front (rather than relying solely on `.with_environments`'s
    /// internal, silently-degrading resolution) so an unknown env surfaces a
    /// clear error here instead of a confusing OAuth failure later.
    fn provider_for(&self, env: &str) -> Result<PkceAuthProvider> {
        let resolved = environments::resolve(env)?;
        Ok(build_provider(&resolved))
    }
}

/// Builds a `PkceAuthProvider` named `"godaddy"` (not `env` — see
/// [`GoDaddyAuthProvider`]'s doc) from an already-resolved (and
/// gddy-specific-derived) environment.
fn build_provider(env: &GddyEnvConfig) -> PkceAuthProvider {
    log_resolved_oauth(env);
    PkceAuthProvider::new(
        "godaddy",
        env.auth_url.clone(),
        env.token_url.clone(),
        env.client_id.clone(),
        environments::DEFAULT_OAUTH_SCOPES,
    )
    .with_app_id(environments::APP_ID)
    .with_redirect_uri(environments::REDIRECT_URI)
    .with_environments(Arc::clone(environments::instance()))
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
/// It mirrors `GddyEnvConfig`'s own app-scoped `GDDY_AUTH_URL`/`GDDY_TOKEN_URL`
/// overrides so the logged values are what's actually sent — and flags when a
/// value comes from an env var rather than config, which is the usual cause of
/// a "wrong client id". No secrets are logged (the OAuth client id is a public
/// identifier; tokens never pass through here).
///
/// Enable with `RUST_LOG=gddy=debug` (e.g. `RUST_LOG=gddy=debug gddy domain
/// available example.com --env dev`).
fn log_resolved_oauth(env: &GddyEnvConfig) {
    if !tracing::enabled!(tracing::Level::DEBUG) {
        return;
    }
    // App-scoped, not environment-scoped — matches `GddyEnvConfig`'s own
    // `env_config(env = "AUTH_URL"/"TOKEN_URL")` attributes. `client_id` has
    // no such attribute (no env-var override exists for it), so it's always
    // just `env.client_id` below. A blank value is treated as absent, same
    // as `EnvConfig`'s own resolution.
    let prefix = environments::env_prefix(environments::APP_ID);
    let override_var = |suffix: &str| {
        std::env::var(format!("{prefix}_{suffix}"))
            .ok()
            .filter(|v| !v.trim().is_empty())
    };
    let auth_url_ovr = override_var("AUTH_URL");
    let token_url_ovr = override_var("TOKEN_URL");
    tracing::debug!(
        env = %env.name,
        client_id = %env.client_id,
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
        let provider = self.provider_for(env)?;
        provider.get_credential(env, command, tier).await
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
        // List every environment that could plausibly have a cached
        // credential — built-ins + locally-configured envs (env-var-only
        // envs are excluded from `listable`, matching the `env list`
        // contract) plus anything with a registered PAT. `listable` falls
        // back to built-ins on a malformed local config, so this never fails
        // wholesale.
        //
        // `PkceAuthProvider::list_environments` can't help here: keyring and
        // file-fallback storage aren't enumerable by prefix, so it only
        // reflects its own in-memory token cache — and since `provider_for`
        // builds a fresh provider per call, that cache is always empty. So
        // rather than ask providers to enumerate, list every *known*
        // environment name and let `Dispatcher::all_statuses` call `status`
        // on each; `status` does read real persisted storage, so a
        // "not logged in" result there is trustworthy in a way an empty
        // enumeration result is not.
        let listable = environments::listable()?;
        let mut envs: std::collections::BTreeSet<String> =
            listable.into_iter().map(|resolved| resolved.name).collect();
        match pat::registry_envs().await {
            Ok(pats) => envs.extend(pats),
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    "failed to load PAT registry while listing environments; continuing"
                );
            }
        }
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
