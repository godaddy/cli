//! The authenticated Commerce Stores client and the mapping of its failures to
//! coded CLI errors.

use cli_engine::{CliCoreError, CommandContext, Result};
use commerce_client::{Client, Error, client_with_builder, types::V1Store};
use uuid::Uuid;

use crate::{
    commerce::store::find_your_store_id_hint,
    environments,
    error::GddyError,
    http,
    pat::{PAT_ENV_VAR, env_var_for},
};

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
        Ok(response) => require_store_record(response.into_inner(), store_id),
        Err(Error::UnexpectedResponse(response)) => {
            Err(http_error(response.status().as_u16(), store_id, env))
        }
        Err(Error::CommunicationError(error)) => Err(GddyError::network(error.to_string())
            .with_system(SYSTEM)
            .into_cli_error()),
        // The decode error is not shown: serde names the offending value, and a
        // store record carries government IDs, addresses and payment details.
        Err(Error::InvalidResponsePayload(..)) => Err(GddyError::unexpected(
            "the Commerce API returned a store this CLI could not read",
        )
        .with_system(SYSTEM)
        .into_cli_error()),
        Err(error) => Err(
            GddyError::unexpected(format!("the store request failed: {error}"))
                .with_system(SYSTEM)
                .into_cli_error(),
        ),
    }
}

/// Rejects a successful response that is not the store that was asked for. The
/// generated store type has no required fields, so an empty object, an
/// error-shaped one, or one whose fields are all blank decodes without complaint.
/// Showing that as a store would invent a summary from the id that was asked for,
/// and `use` would save an id the API never confirmed.
///
/// A record counts as a store when it has a non-blank id, business id or name,
/// even a sparse one. If it carries an id, that id must be the one requested:
/// a response about a different store is not an answer, and saving the requested
/// id on the strength of it would select a store the API did not identify.
fn require_store_record(store: V1Store, requested: &str) -> Result<V1Store> {
    let has_text = |value: Option<&str>| value.is_some_and(|text| !text.trim().is_empty());
    let id = store.id.as_deref().map(String::as_str);
    if !(has_text(id)
        || has_text(store.business_id.as_deref())
        || has_text(store.display_name.as_deref()))
    {
        return Err(GddyError::unexpected(
            "the Commerce API answered without a store: the response had no id, business \
             id or name",
        )
        .with_system(SYSTEM)
        .into_cli_error());
    }
    if has_text(id) && !same_store_id(id.unwrap_or_default(), requested) {
        return Err(GddyError::unexpected(
            "the Commerce API answered with a different store than the one requested",
        )
        .with_system(SYSTEM)
        .into_cli_error());
    }
    Ok(store)
}

/// Whether `returned` names the same store as `requested`, ignoring case and
/// hyphenation the way store ids are canonicalized elsewhere. A returned id that
/// is not a store id at all is not the same store.
fn same_store_id(returned: &str, requested: &str) -> bool {
    match (Uuid::parse_str(returned.trim()), Uuid::parse_str(requested)) {
        (Ok(returned), Ok(requested)) => returned == requested,
        _ => false,
    }
}

/// What to try when the account may not read the store in `env`. A personal
/// access token overrides a browser login and carries its own fixed permissions,
/// so the advice covers both ways one can be in effect: exported in the shell,
/// which `gddy pat remove` does not touch, and saved with `gddy pat add`, which
/// it does, for one environment at a time.
fn forbidden_fix(env: &str) -> String {
    let per_env = env_var_for(env);
    format!(
        "Sign in with an account that has access to the store. After a browser \
        login, `gddy auth status --fields scopes` shows the permissions it \
        holds. A personal access token takes precedence over a browser login \
        and carries its own fixed permissions. If {per_env} or {PAT_ENV_VAR} is \
        set in your shell, unset it, or replace it with a token that has the \
        permissions this command needs. A token saved with `gddy pat add` is \
        removed with: gddy pat remove --env {env}"
    )
}

/// Maps a failed store read to a coded error. A 404 and a 403 name the store and
/// say what to try, since "HTTP error 404" alone does not tell a merchant whether
/// the id, the environment or their account is the problem.
///
/// The response body is never put in the error. Store responses can carry
/// government IDs, addresses and payment details, and an error is printed, logged
/// and pasted into bug reports, so it names only the status.
///
/// The catalog client maps its HTTP failures the same way, so the wording does
/// not assume the store is being read.
pub(super) fn http_error(status: u16, store_id: &str, env: &str) -> CliCoreError {
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
            format!("this account is not allowed to use store {store_id}"),
            SYSTEM,
        )
        .with_fix(forbidden_fix(env)),
        _ => GddyError::from_http(status, "", SYSTEM),
    }
    .into_cli_error()
}

#[cfg(test)]
mod tests;
