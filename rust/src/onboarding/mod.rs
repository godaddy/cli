mod client;
mod flow;
mod prompt;
mod types;

pub use client::{OnboardingClient, OnboardingError};
pub use flow::{AgreementDecision, decide};
pub use prompt::prompt_accept_agreements;
pub use types::{CliOnboardingResult, OnboardingStatus};
