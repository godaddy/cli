//! The authenticated Commerce Stores client and the mapping of its failures to
//! coded CLI errors.

use cli_engine::{CliCoreError, CommandContext, Result};
use commerce_client::{Client, Error, client_with_builder, types::V1Store};
use uuid::Uuid;

use crate::{commerce::store::find_your_store_id_hint, environments, error::GddyError, http};

/// Name the Stores API is reported under in error envelopes.
const SYSTEM: &str = "commerce";

/// A Commerce client for the active environment, authenticated with a token that
/// carries `scopes` (prompting for consent if the login lacks them).
pub(crate) async fn make_client(ctx: &CommandContext, scopes: &[&str]) -> Result<Client> {
    http::ensure_generated_client_transport_observer_registered();
    let required: Vec<String> = scopes.iter().map(|scope| (*scope).to_owned()).collect();
    let token = ctx.credential_with_scopes(&required).await?.token;
    let base_url = environments::resolve(&ctx.middleware.env)?.api_url;
    client_with_builder(
        http::api_client_builder(),
        &base_url,
        &format!("Bearer {token}"),
        &Uuid::new_v4().to_string(),
    )
    .map_err(|error| {
        GddyError::config(format!("failed to construct the Commerce client: {error}"))
            .with_system(SYSTEM)
            .into_cli_error()
    })
}

/// Reads one store through the v1 operation. The v2 read is not routed yet, and
/// v1 returns the same store, so this is the one read every store command uses.
///
/// `store_id` must already be a canonical store id; `env` only shapes the error
/// text and fix for a store that is not there.
pub(crate) async fn fetch_store(client: &Client, store_id: &str, env: &str) -> Result<V1Store> {
    match client
        .get_store_by_id_v1()
        .store_id(store_id.to_owned())
        .send()
        .await
    {
        Ok(response) => Ok(response.into_inner()),
        Err(Error::UnexpectedResponse(response)) => {
            let status = response.status().as_u16();
            let body = response.text().await.unwrap_or_default();
            Err(http_error(status, &body, store_id, env))
        }
        Err(Error::CommunicationError(error)) => Err(GddyError::network(error.to_string())
            .with_system(SYSTEM)
            .into_cli_error()),
        Err(Error::InvalidResponsePayload(_, error)) => Err(GddyError::unexpected(format!(
            "the Commerce API returned a store this CLI could not read: {error}"
        ))
        .with_system(SYSTEM)
        .into_cli_error()),
        Err(error) => Err(
            GddyError::unexpected(format!("the store request failed: {error}"))
                .with_system(SYSTEM)
                .into_cli_error(),
        ),
    }
}

/// Maps a failed store read to a coded error. A 404 and a 403 name the store and
/// say what to try, since "HTTP error 404" alone does not tell a merchant whether
/// the id, the environment or their account is the problem.
fn http_error(status: u16, body: &str, store_id: &str, env: &str) -> CliCoreError {
    match status {
        404 => GddyError::from_status(
            404,
            format!("no store {store_id} was found in the {env} environment"),
            SYSTEM,
        )
        .with_fix(format!(
            "Check the store id, and that --env matches where the store lives. {}",
            find_your_store_id_hint(env)
        )),
        403 => GddyError::from_status(
            403,
            format!("this account is not allowed to read store {store_id}"),
            SYSTEM,
        )
        .with_fix(
            "Sign in with an account that has access to the store, or check \
            which accounts the store is shared with. To confirm the permissions \
            this login holds, run: gddy auth scope",
        ),
        _ => GddyError::from_http(status, body, SYSTEM),
    }
    .into_cli_error()
}

#[cfg(test)]
mod tests;
