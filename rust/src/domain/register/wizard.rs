//! Wizard step-runner: manages forward/back navigation, state, and the step
//! header display for the domain registration wizard.

use cli_engine::{Credential, Result};
use console::style;

use super::steps;

/// The result of running a single wizard step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StepResult {
    /// Advance to the next step.
    Continue,
    /// Go back to the previous step (or restart the current step if at step 0).
    Back,
    /// The user cancelled the wizard.
    Cancel,
}

/// Shared context passed to each wizard step.
pub(crate) struct StepContext {
    pub credential: Credential,
    pub env: String,
    pub debug: bool,
}

/// Accumulated state across all wizard steps.
#[derive(Debug, Clone, Default)]
pub(crate) struct WizardState {
    // Step 1: Discovery
    pub domain: Option<String>,
    pub available: bool,

    // Step 2: Options
    pub period: u64,
    pub privacy: bool,
    pub auto_renew: bool,
    pub nameservers: Vec<String>,

    // Step 3: Contacts
    pub contacts: ContactsChoice,

    // Step 4: Review (populated after quote)
    pub quote_token: Option<String>,
    pub price: Option<String>,
    pub currency: Option<String>,
    pub agreement_titles: Vec<String>,
    pub agreement_types: Vec<String>,

    // Step 5: Execute (populated after registration)
    pub status: Option<String>,
    pub operation_id: Option<String>,

    // Set when the wizard is cancelled by the user (not an error).
    pub cancelled: bool,
    // Set when the user navigated back past the entry step (not a cancellation).
    pub backed_out: bool,
}

/// How contacts are supplied for the registration.
#[derive(Debug, Clone, Default)]
pub(crate) enum ContactsChoice {
    /// Use the account's default contacts (omit from request).
    #[default]
    AccountDefault,
    /// Use contacts loaded from contacts.toml.
    FromFile(crate::contacts::ContactsFile),
    /// Contacts entered interactively during the wizard.
    Manual(crate::contacts::ContactsFile),
}

impl WizardState {
    pub fn new() -> Self {
        Self {
            period: 1,
            privacy: true,
            auto_renew: true,
            ..Default::default()
        }
    }

    /// Pre-populate from CLI flags for non-interactive fallback or partial entry.
    pub fn with_domain(mut self, domain: Option<String>) -> Self {
        self.domain = domain;
        self
    }

    pub fn with_period(mut self, period: u64) -> Self {
        self.period = period;
        self
    }

    pub fn with_privacy(mut self, privacy: bool) -> Self {
        self.privacy = privacy;
        self
    }

    pub fn with_auto_renew(mut self, auto_renew: bool) -> Self {
        self.auto_renew = auto_renew;
        self
    }

    pub fn with_nameservers(mut self, nameservers: Vec<String>) -> Self {
        self.nameservers = nameservers;
        self
    }
}

/// Step metadata for the header display.
struct StepInfo {
    name: &'static str,
}

const STEPS: &[StepInfo] = &[
    StepInfo { name: "Discovery" },
    StepInfo { name: "Options" },
    StepInfo { name: "Contacts" },
    StepInfo {
        name: "Review & Confirm",
    },
    StepInfo { name: "Register" },
];

/// Run the wizard starting at `start_at` step (0-indexed).
///
/// Returns the final `WizardState` on success, or an error if the wizard is
/// cancelled or a step fails.
///
/// When entered mid-flow (e.g. from `domain suggest` at Options), the header
/// counts only remaining steps — `Step 1/4: Options` rather than `Step 2/5` —
/// so the counter matches the work the user still has to do.
pub(crate) async fn run_wizard(
    mut state: WizardState,
    ctx: StepContext,
    start_at: usize,
) -> Result<WizardState> {
    let total_steps = STEPS.len();
    let mut current = start_at;
    // Relative denominator: how many steps this entry point will show.
    let display_total = total_steps.saturating_sub(start_at);

    loop {
        if current >= total_steps {
            break;
        }

        let step = &STEPS[current];
        let display_num = current.saturating_sub(start_at) + 1;
        eprintln!(
            "\n  {} Step {}/{}: {}",
            style("─").dim(),
            display_num,
            display_total,
            style(step.name).bold()
        );

        let step_result = match current {
            0 => steps::discovery::run(&mut state, &ctx).await,
            1 => steps::options::run(&mut state, &ctx).await,
            2 => steps::contacts::run(&mut state, &ctx).await,
            3 => steps::review::run(&mut state, &ctx).await,
            4 => steps::execute::run(&mut state, &ctx).await,
            _ => unreachable!(),
        };

        let result = match step_result {
            Ok(r) => r,
            Err(e) if is_prompt_cancelled(&e) => {
                eprintln!(
                    "\n  {} Interrupted. No charges were made.",
                    style("✗").red().bold()
                );
                state.cancelled = true;
                return Ok(state);
            }
            Err(e) => return Err(e),
        };

        match result {
            StepResult::Continue => {
                current += 1;
            }
            StepResult::Back => {
                if current == start_at {
                    // Already at the entry point — can't go further back.
                    // Signal that we backed out so the caller (bridge) can
                    // re-show its own selection UI.
                    state.backed_out = true;
                    return Ok(state);
                }
                current -= 1;
                // If navigating back to Discovery, clear domain state so the
                // step re-prompts for a domain name instead of short-circuiting.
                if current == 0 {
                    state.domain = None;
                    state.available = false;
                }
            }
            StepResult::Cancel => {
                eprintln!(
                    "\n  {} Wizard cancelled. No charges were made.",
                    style("✗").red().bold()
                );
                state.cancelled = true;
                return Ok(state);
            }
        }
    }

    Ok(state)
}

/// Detect if an error came from a cancelled prompt (Ctrl+C or EOF in dialoguer).
fn is_prompt_cancelled(err: &cli_engine::CliCoreError) -> bool {
    let msg = err.to_string();
    msg.contains("prompt cancelled") || msg.contains("interrupted")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wizard_state_defaults_are_sensible() {
        let state = WizardState::new();
        assert_eq!(state.period, 1);
        assert!(state.privacy);
        assert!(state.auto_renew);
        assert!(state.domain.is_none());
        assert!(state.nameservers.is_empty());
    }

    #[test]
    fn wizard_state_builder_methods_work() {
        let state = WizardState::new()
            .with_domain(Some("example.com".to_string()))
            .with_period(2)
            .with_privacy(false)
            .with_auto_renew(false)
            .with_nameservers(vec!["ns1.example.com".to_string()]);
        assert_eq!(state.domain.as_deref(), Some("example.com"));
        assert_eq!(state.period, 2);
        assert!(!state.privacy);
        assert!(!state.auto_renew);
        assert_eq!(state.nameservers, vec!["ns1.example.com"]);
    }

    #[test]
    fn step_result_equality() {
        assert_eq!(StepResult::Continue, StepResult::Continue);
        assert_eq!(StepResult::Back, StepResult::Back);
        assert_eq!(StepResult::Cancel, StepResult::Cancel);
        assert_ne!(StepResult::Continue, StepResult::Cancel);
    }

    #[test]
    fn steps_metadata_has_expected_count() {
        assert_eq!(STEPS.len(), 5);
        assert_eq!(STEPS[0].name, "Discovery");
        assert_eq!(STEPS[1].name, "Options");
        assert_eq!(STEPS[2].name, "Contacts");
        assert_eq!(STEPS[3].name, "Review & Confirm");
        assert_eq!(STEPS[4].name, "Register");
    }

    #[test]
    fn mid_flow_step_counter_is_relative_to_entry_point() {
        // From suggest/available the wizard starts at Options (index 1):
        // remaining steps are Options→Contacts→Review→Register → 4 total,
        // and Options itself is display step 1.
        let start_at = 1;
        let display_total = STEPS.len().saturating_sub(start_at);
        assert_eq!(display_total, 4);
        assert_eq!(1usize.saturating_sub(start_at) + 1, 1); // Options → 1/4
        assert_eq!(4usize.saturating_sub(start_at) + 1, 4); // Register → 4/4

        // From quote the wizard starts at Review (index 3): 2 remaining.
        let start_at = 3;
        let display_total = STEPS.len().saturating_sub(start_at);
        assert_eq!(display_total, 2);
        assert_eq!(3usize.saturating_sub(start_at) + 1, 1); // Review → 1/2
        assert_eq!(4usize.saturating_sub(start_at) + 1, 2); // Register → 2/2
    }

    #[test]
    fn wizard_state_carries_all_fields_through_lifecycle() {
        let mut state = WizardState::new()
            .with_domain(Some("test.io".to_string()))
            .with_period(3);

        state.available = true;
        state.quote_token = Some("qt-123".to_string());
        state.price = Some("29.99".to_string());
        state.currency = Some("USD".to_string());
        state.agreement_titles = vec!["ICANN Registrant".to_string()];
        state.agreement_types = vec!["DNRA".to_string()];
        state.status = Some("COMPLETED".to_string());
        state.operation_id = Some("op-456".to_string());

        assert_eq!(state.domain.as_deref(), Some("test.io"));
        assert_eq!(state.period, 3);
        assert!(state.available);
        assert_eq!(state.quote_token.as_deref(), Some("qt-123"));
        assert_eq!(state.price.as_deref(), Some("29.99"));
        assert_eq!(state.currency.as_deref(), Some("USD"));
        assert_eq!(state.agreement_titles.len(), 1);
        assert_eq!(state.agreement_types.len(), 1);
        assert_eq!(state.status.as_deref(), Some("COMPLETED"));
        assert_eq!(state.operation_id.as_deref(), Some("op-456"));
    }
}
