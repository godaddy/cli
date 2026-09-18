//! Cross-cutting HTTP helpers shared across command groups — not tied to any
//! one product API. Kept separate from any single command's `common.rs`
//! because callers span multiple top-level modules (`email`, `hosting`,
//! `platform`, `api`).

use reqwest::Client;

const USER_AGENT: &str = concat!("godaddy-cli/", env!("CARGO_PKG_VERSION"));

/// Builds a reqwest Client with the standard GoDaddy CLI User-Agent.
pub fn make_http_client() -> Client {
    Client::builder()
        .user_agent(USER_AGENT)
        .build()
        .expect("failed to build HTTP client")
}

/// Bridges `generated-client-support::TransportObserver` (shared by every
/// progenitor-generated client crate: `domains-client`, `shopping-client`,
/// `email-client`, ...) into cli-engine's `--debug transport` logger. One
/// registration for every generated client, rather than one per command
/// module — they'd all just forward to the same
/// `cli_engine::transport::debug_log_reqwest_*` calls anyway.
struct CliEngineTransportObserver;

impl generated_client_support::TransportObserver for CliEngineTransportObserver {
    fn on_request(&self, request: &reqwest::Request) {
        cli_engine::transport::debug_log_reqwest_request(request);
    }

    fn on_response(&self, status: reqwest::StatusCode, headers: &reqwest::header::HeaderMap) {
        cli_engine::transport::debug_log_reqwest_response(status, headers, &[]);
    }
}

static GENERATED_CLIENT_TRANSPORT_OBSERVER_INIT: std::sync::Once = std::sync::Once::new();

/// Registers the shared transport observer the first time any generated
/// client is constructed. Idempotent and cheap to call from every
/// `make_client`-style helper (`domain`, `email`, `shopping`, ...).
pub(crate) fn ensure_generated_client_transport_observer_registered() {
    GENERATED_CLIENT_TRANSPORT_OBSERVER_INIT.call_once(|| {
        generated_client_support::set_transport_observer(Some(std::sync::Arc::new(
            CliEngineTransportObserver,
        )));
    });
}

/// The API base URL for `env`.
///
/// # Errors
///
/// Returns an error when `env` fails to resolve — for example a malformed
/// `environments.toml` override. Propagated rather than silently retrying
/// against a different environment or a hardcoded default: `env` is
/// normally already validated (by `--env` parsing or the persisted active
/// environment), so a failure here means the environment's *config* is
/// broken, and every other `environments::resolve` consumer in this crate
/// treats that as a hard error rather than something to paper over.
pub fn api_url_for_env(env: &str) -> cli_engine::Result<String> {
    crate::environments::resolve(env).map(|e| e.api_url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_url_for_builtins_resolve_to_a_url() {
        // The exact host mapping is covered deterministically in
        // `environments::tests`. Here we only assert the built-ins resolve to a
        // URL — a dev machine may legitimately override a built-in's URL via
        // env var / local config, so don't hard-code the host.
        //
        // `environments::resolve` validates every field, so this races
        // against any test elsewhere in the crate that mutates a GDDY_*
        // override var — see `ENV_LOCK`'s doc.
        let _g = crate::environments::test_support::ENV_LOCK.blocking_lock();
        for env in ["prod", "ote"] {
            let url = api_url_for_env(env).expect("resolves");
            assert!(url.contains("://"), "{env} -> {url:?}");
        }
    }

    #[test]
    fn api_url_for_env_rejects_an_unknown_env() {
        // An unrecognized env is a hard error now, not a silent fallback to
        // some other environment's URL.
        let err = api_url_for_env("definitely-not-a-real-env-xyz")
            .expect_err("unknown env must not resolve");
        assert!(err.to_string().contains("definitely-not-a-real-env-xyz"));
    }
}
