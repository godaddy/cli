mod client;
mod ensure;
mod flow;
mod prompt;
mod types;

pub use client::{OnboardingClient, OnboardingError};
pub use ensure::{EnsureOutcome, ensure_ready_for_app_init};
pub use flow::{AgreementDecision, decide};
pub use prompt::prompt_accept_agreements;
pub use types::{CliOnboardingResult, OnboardingStatus};
