//! Step 2: Registration options — period, privacy, auto-renew, custom
//! nameservers.

use cli_engine::{CliCoreError, Result};
use console::style;
use dialoguer::{Confirm, Input, Select};

use super::super::wizard::{StepContext, StepResult, WizardState};

/// Available registration periods (years).
const PERIOD_OPTIONS: &[u64] = &[1, 2, 3, 5, 10];

pub(crate) async fn run(state: &mut WizardState, _ctx: &StepContext) -> Result<StepResult> {
    eprintln!(
        "\n  {} Configuring registration for {}",
        style("⚙").bold(),
        style(state.domain.as_deref().unwrap_or("unknown")).cyan()
    );

    // Period selection.
    let period_labels: Vec<String> = PERIOD_OPTIONS
        .iter()
        .map(|p| {
            if *p == 1 {
                "1 year".to_string()
            } else {
                format!("{p} years")
            }
        })
        .collect();

    let period_idx = Select::new()
        .with_prompt("Registration period")
        .items(&period_labels)
        .default(0)
        .interact()
        .map_err(|e| CliCoreError::message(format!("prompt cancelled: {e}")))?;
    state.period = PERIOD_OPTIONS[period_idx];

    // Privacy protection.
    state.privacy = Confirm::new()
        .with_prompt("Enable WHOIS privacy protection?")
        .default(true)
        .interact()
        .map_err(|e| CliCoreError::message(format!("prompt cancelled: {e}")))?;

    // Auto-renew.
    state.auto_renew = Confirm::new()
        .with_prompt("Enable auto-renewal?")
        .default(true)
        .interact()
        .map_err(|e| CliCoreError::message(format!("prompt cancelled: {e}")))?;

    // Custom nameservers (optional).
    let custom_ns = Confirm::new()
        .with_prompt("Use custom nameservers? (No = GoDaddy defaults)")
        .default(false)
        .interact()
        .map_err(|e| CliCoreError::message(format!("prompt cancelled: {e}")))?;

    if custom_ns {
        state.nameservers = prompt_nameservers()?;
    } else {
        state.nameservers = Vec::new();
    }

    eprintln!(
        "  {} Options configured: {} year(s), privacy={}, auto-renew={}",
        style("✓").green().bold(),
        state.period,
        if state.privacy { "on" } else { "off" },
        if state.auto_renew { "on" } else { "off" },
    );

    Ok(StepResult::Continue)
}

fn prompt_nameservers() -> Result<Vec<String>> {
    let mut nameservers = Vec::new();
    eprintln!("  Enter nameservers (empty line to finish, min 2):");
    loop {
        let ns: String = Input::new()
            .with_prompt(format!("  NS {}", nameservers.len() + 1))
            .allow_empty(true)
            .interact_text()
            .map_err(|e| CliCoreError::message(format!("prompt cancelled: {e}")))?;

        if ns.is_empty() {
            if nameservers.len() < 2 && !nameservers.is_empty() {
                eprintln!("  At least 2 nameservers are required.");
                continue;
            }
            break;
        }

        // Validate nameserver format.
        match crate::domain::common::validate_domain_name(&ns) {
            Ok(valid) => nameservers.push(valid),
            Err(e) => {
                eprintln!("  Invalid nameserver: {e}");
                continue;
            }
        }
    }
    Ok(nameservers)
}
