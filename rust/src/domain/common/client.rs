use cli_engine::{CliCoreError, CommandContext, Credential, Result};

use crate::environments;

const USER_AGENT: &str = concat!("godaddy-cli/", env!("CARGO_PKG_VERSION"));

/// Bridges domains-client's request/response observations into cli-engine's
/// `--debug transport` logger. domains-client defines the `TransportObserver`
/// extension point itself and has no compile-time dependency on cli-engine —
/// this crate *pushes* the logging behavior in, rather than domains-client
/// *pulling* it from the engine.
struct CliEngineTransportObserver;

impl domains_client::TransportObserver for CliEngineTransportObserver {
    fn on_request(&self, request: &reqwest::Request) {
        cli_engine::transport::debug_log_reqwest_request(request);
    }

    fn on_response(&self, status: reqwest::StatusCode, headers: &reqwest::header::HeaderMap) {
        cli_engine::transport::debug_log_reqwest_response(status, headers, &[]);
    }
}

static TRANSPORT_OBSERVER_INIT: std::sync::Once = std::sync::Once::new();

fn ensure_transport_observer_registered() {
    TRANSPORT_OBSERVER_INIT.call_once(|| {
        domains_client::set_transport_observer(Some(std::sync::Arc::new(
            CliEngineTransportObserver,
        )));
    });
}
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
    ensure_transport_observer_registered();
    let config = environments::resolve(env)?;
    let authorization = format!("Bearer {}", cred.token);
    let request_id = uuid::Uuid::new_v4().to_string();
    domains_client::client_with_auth(
        &config.domains_api_url,
        &authorization,
        USER_AGENT,
        &request_id,
    )
    .map_err(|e| CliCoreError::message(format!("failed to build domains client: {e}")))
}
