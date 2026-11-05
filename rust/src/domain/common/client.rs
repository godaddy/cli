use cli_engine::{CliCoreError, CommandContext, Credential, Result};

use crate::environments;

/// Build a Domains API client for the active environment, authenticating with
/// the resolved OAuth bearer token.
pub(crate) async fn make_client(ctx: &CommandContext) -> Result<domains_client::Client> {
    let cred = ctx.credential().await?;
    make_client_with_cred(&ctx.middleware.env, &cred)
}

/// Build the Domains API client from an already-resolved credential, so callers
/// that need the credential themselves (e.g. `purchase`, for the consent
/// principal) resolve it once and reuse the same token for the requests.
pub(crate) fn make_client_with_cred(
    env: &str,
    cred: &Credential,
) -> Result<domains_client::Client> {
    crate::http::ensure_generated_client_transport_observer_registered();
    let config = environments::resolve(env)?;
    let authorization = format!("Bearer {}", cred.token);
    let request_id = uuid::Uuid::new_v4().to_string();
    domains_client::client_with_builder(
        crate::http::api_client_builder(),
        &config.domains_api_url,
        &authorization,
        &request_id,
    )
    .map_err(|e| CliCoreError::message(format!("failed to build domains client: {e}")))
}
