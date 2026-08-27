//! Bridge functions allowing other domain commands (suggest, available, quote)
//! to hand off to the registration wizard when running interactively.

use cli_engine::{CliCoreError, CommandResult, Result};
use dialoguer::Confirm;

use super::WizardExit;
use super::wizard::WizardState;

/// After `domain available` finds a domain is available, offer to continue
/// with registration. Returns `None` if the user declines or the wizard is
/// cancelled. Re-asks if the user navigates back from the wizard.
pub(crate) async fn offer_registration_from_available(
    ctx: &cli_engine::CommandContext,
    domain: &str,
    price: Option<String>,
    currency: Option<String>,
) -> Result<Option<CommandResult>> {
    if !ctx.is_interactive() {
        return Ok(None);
    }

    loop {
        let proceed = Confirm::new()
            .with_prompt(format!("Would you like to register {domain}?"))
            .default(false)
            .interact()
            .map_err(|e| CliCoreError::message(format!("prompt cancelled: {e}")))?;

        if !proceed {
            return Ok(None);
        }

        let mut state = WizardState::new().with_domain(Some(domain.to_owned()));
        state.available = true;
        state.price = price.clone();
        state.currency = currency.clone();

        match super::launch_wizard(ctx, state, 1).await? {
            WizardExit::Completed(result) => {
                return Ok(Some(super::present_for_host_command(ctx, result)));
            }
            WizardExit::BackedOut => continue,
            WizardExit::Cancelled => return Ok(None),
        }
    }
}

/// After `domain suggest` displays results, offer to pick one and register.
/// Returns `None` if the user declines or the wizard is cancelled. Re-shows
/// the selection if the user navigates back from the wizard.
pub(crate) async fn offer_registration_from_suggest(
    ctx: &cli_engine::CommandContext,
    suggestions: &[String],
) -> Result<Option<CommandResult>> {
    if !ctx.is_interactive() || suggestions.is_empty() {
        return Ok(None);
    }

    // Show suggestions inline so the user sees what's available before choosing.
    eprintln!("\n  Here are some available domains based on your input:\n");
    for (i, name) in suggestions.iter().enumerate() {
        eprintln!("    {}. {}", i + 1, name);
    }
    eprintln!();

    let mut items: Vec<String> = suggestions.to_vec();
    items.push("(enter a different domain)".to_owned());
    items.push("(skip — just show results)".to_owned());

    loop {
        let selection = dialoguer::Select::new()
            .with_prompt("Would you like to register one of these domains? Select one to proceed")
            .items(&items)
            .default(items.len() - 1)
            .interact()
            .map_err(|e| CliCoreError::message(format!("prompt cancelled: {e}")))?;

        // Last option = skip, return None to let normal output render.
        if selection == items.len() - 1 {
            return Ok(None);
        }

        // Second-to-last = enter a different domain.
        let domain = if selection == items.len() - 2 {
            None
        } else {
            Some(items[selection].clone())
        };

        let mut state = WizardState::new().with_domain(domain.clone());
        let exit = if domain.is_some() {
            state.available = true;
            super::launch_wizard(ctx, state, 1).await?
        } else {
            super::launch_wizard(ctx, state, 0).await?
        };

        match exit {
            WizardExit::Completed(result) => {
                return Ok(Some(super::present_for_host_command(ctx, result)));
            }
            WizardExit::BackedOut => continue,
            WizardExit::Cancelled => return Ok(None),
        }
    }
}

/// After `domain quote` prices a domain, offer to purchase it directly.
/// Returns `None` if the user declines or the wizard is cancelled. Re-asks if
/// the user navigates back from the wizard.
pub(crate) async fn offer_registration_from_quote(
    ctx: &cli_engine::CommandContext,
    domain: &str,
    quote_token: &str,
    price: Option<String>,
    currency: Option<String>,
    period: u64,
) -> Result<Option<CommandResult>> {
    if !ctx.is_interactive() {
        return Ok(None);
    }

    loop {
        let proceed = Confirm::new()
            .with_prompt(format!("Would you like to purchase {domain} now?"))
            .default(false)
            .interact()
            .map_err(|e| CliCoreError::message(format!("prompt cancelled: {e}")))?;

        if !proceed {
            return Ok(None);
        }

        let mut state = WizardState::new()
            .with_domain(Some(domain.to_owned()))
            .with_period(period);
        state.available = true;
        state.quote_token = Some(quote_token.to_owned());
        state.price = price.clone();
        state.currency = currency.clone();

        // Start at step 3 (Review & Confirm) since quote is already done.
        match super::launch_wizard(ctx, state, 3).await? {
            WizardExit::Completed(result) => {
                return Ok(Some(super::present_for_host_command(ctx, result)));
            }
            WizardExit::BackedOut => continue,
            WizardExit::Cancelled => return Ok(None),
        }
    }
}
