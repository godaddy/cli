pub mod client;
mod commands;
pub mod public_url;

use cli_engine::RuntimeGroupSpec;

/// The app command group, composed below `gddy platform`.
pub fn group() -> RuntimeGroupSpec {
    commands::application_group()
}
