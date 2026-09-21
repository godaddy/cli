mod client;
mod commands;
mod extension;
mod onboarding;
mod public_url;

use cli_engine::RuntimeGroupSpec;

/// The app command group, composed below `gddy platform`.
pub fn group() -> RuntimeGroupSpec {
    commands::application_group()
}
