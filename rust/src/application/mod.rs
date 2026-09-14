pub mod client;
mod commands;
pub(crate) mod native_app_client;
pub mod public_url;

use cli_engine::RuntimeGroupSpec;

/// The app command group, composed below `gddy platform`.
pub fn group() -> RuntimeGroupSpec {
    commands::application_group()
}
