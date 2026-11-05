mod client;
mod commands;
mod extension;
pub(crate) mod native_app_client;
mod onboarding;
mod public_url;

use cli_engine::RuntimeGroupSpec;

/// The app command group, composed below `gddy platform`.
pub fn group() -> RuntimeGroupSpec {
    commands::application_group()
}
