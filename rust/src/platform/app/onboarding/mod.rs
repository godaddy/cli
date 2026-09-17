mod client;
mod ensure;
mod flow;
mod prompt;
mod types;

pub(crate) use client::OnboardingClient;
pub use ensure::ensure_ready_for_app_init;
// Only consumed by this module family's own `#[cfg(test)]` code
// (`flow.rs`/`client.rs` tests reach it via this path, not `super::types`
// directly) — gated the same way to avoid an unused-import warning on a
// non-test build.
#[cfg(test)]
pub use types::{CliOnboardingResult, OnboardingStatus};
