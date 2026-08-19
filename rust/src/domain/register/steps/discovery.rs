//! Step 1: Domain discovery — prompt for a domain name, check availability,
//! and offer alternatives when the requested name is taken.

use cli_engine::{CliCoreError, Result};
use console::style;
use dialoguer::{Input, Select};

use crate::domain::common::{
    api_error, make_client_with_cred, term_for_period, validate_domain_name,
};

use super::super::wizard::{StepContext, StepResult, WizardState};

/// Maximum suggestions to show when a domain is taken.
const MAX_SUGGESTIONS: usize = 10;

pub(crate) async fn run(state: &mut WizardState, ctx: &StepContext) -> Result<StepResult> {
    // If we already have a domain from a previous step or CLI arg, skip prompting.
    if state.domain.is_some() && state.available {
        return Ok(StepResult::Continue);
    }

    let domain = match &state.domain {
        Some(d) => d.clone(),
        None => prompt_domain_name()?,
    };

    let client = make_client_with_cred(&ctx.env, &ctx.credential)?;
    let debug = ctx.debug;

    // Check availability.
    let availability = match client
        .get_domain_availability()
        .domain(domain.as_str())
        .send()
        .await
    {
        Ok(r) => r.into_inner(),
        Err(e) => return Err(api_error("domain availability check", debug, e).await),
    };

    let available = availability.available.unwrap_or(false);
    state.domain = Some(domain.clone());

    if available {
        state.available = true;
        let prices = availability.prices.unwrap_or_default();
        if let Some(tp) = term_for_period(&prices, 1) {
            state.price = tp
                .price
                .as_ref()
                .and_then(crate::domain::common::format_money);
            state.currency = tp
                .price
                .as_ref()
                .and_then(|p| p.currency_code.as_ref())
                .map(|c| c.0.clone());
        }
        eprintln!(
            "  {} {} is available!",
            style("✓").green().bold(),
            style(&domain).cyan().bold()
        );
        return Ok(StepResult::Continue);
    }

    // Domain is taken — offer suggestions.
    eprintln!(
        "  {} {} is not available.",
        style("✗").red().bold(),
        style(&domain).cyan()
    );

    let suggestions = fetch_suggestions(&client, &domain, debug).await?;
    if suggestions.is_empty() {
        eprintln!("  No alternative suggestions found.");
        return prompt_retry_or_cancel(state);
    }

    select_from_suggestions(state, &suggestions)
}

fn prompt_domain_name() -> Result<String> {
    let input: String = Input::new()
        .with_prompt("Domain name to register")
        .validate_with(|input: &String| -> std::result::Result<(), String> {
            validate_domain_name(input)
                .map(|_| ())
                .map_err(|e| e.to_string())
        })
        .interact_text()
        .map_err(|e| CliCoreError::message(format!("prompt cancelled: {e}")))?;
    validate_domain_name(&input)
}

async fn fetch_suggestions(
    client: &domains_client::Client,
    domain: &str,
    debug: bool,
) -> Result<Vec<SuggestionEntry>> {
    let page_size =
        std::num::NonZeroI64::new(MAX_SUGGESTIONS as i64).expect("MAX_SUGGESTIONS is non-zero");
    let resp = match client
        .suggest_domains()
        .query(domain)
        .page_size(page_size)
        .send()
        .await
    {
        Ok(r) => r.into_inner(),
        Err(e) => return Err(api_error("domain suggestion", debug, e).await),
    };

    let mut all: Vec<SuggestionEntry> = Vec::new();
    for item in &resp.items {
        if all.len() >= MAX_SUGGESTIONS {
            break;
        }
        let Some(name) = item.domain.as_deref() else {
            continue;
        };
        if all.iter().any(|s| s.domain == name) {
            continue;
        }
        let prices = item.prices.as_deref().unwrap_or_default();
        let price = term_for_period(prices, 1)
            .and_then(|tp| tp.price.as_ref())
            .and_then(crate::domain::common::format_money);
        let currency = term_for_period(prices, 1)
            .and_then(|tp| tp.price.as_ref())
            .and_then(|p| p.currency_code.as_ref())
            .map(|c| c.0.clone());
        all.push(SuggestionEntry {
            domain: name.to_owned(),
            price,
            currency,
        });
    }

    Ok(all)
}

fn select_from_suggestions(
    state: &mut WizardState,
    suggestions: &[SuggestionEntry],
) -> Result<StepResult> {
    let items: Vec<String> = suggestions
        .iter()
        .map(|s| match (&s.price, &s.currency) {
            (Some(p), Some(c)) => format!("{} ({} {})", s.domain, p, c),
            (Some(p), None) => format!("{} ({})", s.domain, p),
            _ => s.domain.clone(),
        })
        .chain(std::iter::once("↩ Try a different domain".to_string()))
        .chain(std::iter::once("✗ Cancel".to_string()))
        .collect();

    let selection = Select::new()
        .with_prompt("Choose a domain")
        .items(&items)
        .default(0)
        .interact()
        .map_err(|e| CliCoreError::message(format!("prompt cancelled: {e}")))?;

    if selection == items.len() - 1 {
        return Ok(StepResult::Cancel);
    }
    if selection == items.len() - 2 {
        state.domain = None;
        state.available = false;
        return Ok(StepResult::Back);
    }

    let chosen = &suggestions[selection];
    state.domain = Some(chosen.domain.clone());
    state.available = true;
    state.price = chosen.price.clone();
    state.currency = chosen.currency.clone();
    eprintln!(
        "  {} Selected {}",
        style("✓").green().bold(),
        style(&chosen.domain).cyan().bold()
    );
    Ok(StepResult::Continue)
}

fn prompt_retry_or_cancel(state: &mut WizardState) -> Result<StepResult> {
    let items = vec!["Try a different domain", "Cancel"];
    let selection = Select::new()
        .with_prompt("What would you like to do?")
        .items(&items)
        .default(0)
        .interact()
        .map_err(|e| CliCoreError::message(format!("prompt cancelled: {e}")))?;

    if selection == 0 {
        state.domain = None;
        state.available = false;
        Ok(StepResult::Back)
    } else {
        Ok(StepResult::Cancel)
    }
}

struct SuggestionEntry {
    domain: String,
    price: Option<String>,
    currency: Option<String>,
}
