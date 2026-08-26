//! Bridge functions allowing other domain commands (suggest, available, quote)
//! to hand off to the registration wizard when running interactively.

use cli_engine::{CommandResult, Result};
use dialoguer::Confirm;

use super::wizard::WizardState;

/// After `domain available` finds a domain is available, offer to continue
/// with registration. Returns `None` if the user declines.
pub(crate) async fn offer_registration_from_available(
    ctx: &cli_engine::CommandContext,
    domain: &str,
    price: Option<String>,
    currency: Option<String>,
) -> Result<Option<CommandResult>> {
    if !ctx.is_interactive() {
        return Ok(None);
    }

    let proceed = Confirm::new()
        .with_prompt(format!("Would you like to register {domain}?"))
        .default(false)
        .interact()
        .unwrap_or(false);

    if !proceed {
        return Ok(None);
    }

    let mut state = WizardState::new().with_domain(Some(domain.to_owned()));
    state.available = true;
    state.price = price;
    state.currency = currency;

    super::launch_wizard(ctx, state, 1).await
}

/// After `domain suggest` displays results, offer to pick one and register.
/// Returns `None` if the user declines.
pub(crate) async fn offer_registration_from_suggest(
    ctx: &cli_engine::CommandContext,
    suggestions: &[String],
) -> Result<Option<CommandResult>> {
    if !ctx.is_interactive() || suggestions.is_empty() {
        return Ok(None);
    }

    // Show suggestions inline so the user sees what's available before choosing.
    eprintln!(
        "\n  Here are some available domains based on your input:\n"
    );
    for (i, name) in suggestions.iter().enumerate() {
        eprintln!("    {}. {}", i + 1, name);
    }
    eprintln!();

    let mut items: Vec<String> = suggestions.to_vec();
    items.push("(enter a different domain)".to_owned());
    items.push("(skip — just show results)".to_owned());

    let selection = dialoguer::Select::new()
        .with_prompt("Would you like to register one of these domains? Select one to proceed")
        .items(&items)
        .default(items.len() - 1)
        .interact()
        .unwrap_or(items.len() - 1);

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
    if domain.is_some() {
        state.available = true;
        super::launch_wizard(ctx, state, 1).await
    } else {
        super::launch_wizard(ctx, state, 0).await
    }
}

/// After `domain quote` prices a domain, offer to purchase it directly.
/// Returns `None` if the user declines.
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

    let proceed = Confirm::new()
        .with_prompt(format!("Would you like to purchase {domain} now?"))
        .default(false)
        .interact()
        .unwrap_or(false);

    if !proceed {
        return Ok(None);
    }

    let mut state = WizardState::new()
        .with_domain(Some(domain.to_owned()))
        .with_period(period);
    state.available = true;
    state.quote_token = Some(quote_token.to_owned());
    state.price = price;
    state.currency = currency;

    // Start at step 3 (Review & Confirm) since quote is already done.
    super::launch_wizard(ctx, state, 3).await
}
