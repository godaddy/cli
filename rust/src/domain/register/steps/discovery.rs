//! Step 1: Domain discovery — prompt for a domain name, check availability,
//! and offer alternatives when the requested name is taken.

use cli_engine::{CliCoreError, Result};
use console::style;
use dialoguer::Select;

use crate::domain::common::{
    api_error, make_client_with_cred, period_price_map, periods_from_prices,
    prompt_validated_domain_name, term_for_period,
};

use crate::retry::with_retry;

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
        None => prompt_validated_domain_name("Enter domain name to register")?,
    };

    let client = make_client_with_cred(&ctx.env, &ctx.credential)?;
    let debug = ctx.debug;

    // Check availability with retry for transient failures.
    let availability = match with_retry("availability check", 3, || {
        let c = &client;
        let d = domain.as_str();
        async move { c.get_domain_availability().domain(d).send().await }
    })
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
        state.available_periods = periods_from_prices(&prices);
        state.period_prices = period_price_map(&prices);
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

async fn fetch_suggestions(
    client: &domains_client::Client,
    domain: &str,
    debug: bool,
) -> Result<Vec<SuggestionEntry>> {
    let page_size =
        std::num::NonZeroI64::new(MAX_SUGGESTIONS as i64).expect("MAX_SUGGESTIONS is non-zero");
    let resp = match with_retry("suggestions", 3, || {
        let c = client;
        let d = domain;
        async move {
            c.suggest_domains()
                .query(d)
                .page_size(page_size)
                .send()
                .await
        }
    })
    .await
    {
        Ok(r) => r.into_inner(),
        Err(e) => return Err(api_error("domain suggestion", debug, e).await),
    };

    Ok(collect_suggestions(&resp.items, MAX_SUGGESTIONS))
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
        state.available_periods.clear();
        state.period_prices.clear();
        return Ok(StepResult::Back);
    }

    let chosen = &suggestions[selection];
    state.domain = Some(chosen.domain.clone());
    state.available = true;
    state.available_periods = chosen.available_periods.clone();
    state.period_prices = chosen.period_prices.clone();
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
        state.available_periods.clear();
        state.period_prices.clear();
        Ok(StepResult::Back)
    } else {
        Ok(StepResult::Cancel)
    }
}

struct SuggestionEntry {
    domain: String,
    price: Option<String>,
    currency: Option<String>,
    available_periods: Vec<u64>,
    period_prices: std::collections::BTreeMap<u64, String>,
}

/// Extract unique suggestions from raw API items, capped at `max`.
/// Factored out for testability.
fn collect_suggestions(
    items: &[domains_client::types::Suggestion],
    max: usize,
) -> Vec<SuggestionEntry> {
    let mut all: Vec<SuggestionEntry> = Vec::new();
    for item in items {
        if all.len() >= max {
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
            available_periods: periods_from_prices(prices),
            period_prices: period_price_map(prices),
        });
    }
    all
}

#[cfg(test)]
mod tests {
    use super::*;
    use domains_client::types::Suggestion;

    fn make_suggestion(domain: &str) -> Suggestion {
        Suggestion {
            domain: Some(domain.to_string()),
            inventory: None,
            prices: None,
        }
    }

    #[test]
    fn collect_suggestions_deduplicates() {
        let items = vec![
            make_suggestion("a.com"),
            make_suggestion("b.com"),
            make_suggestion("a.com"), // duplicate
            make_suggestion("c.com"),
        ];
        let results = collect_suggestions(&items, 10);
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].domain, "a.com");
        assert_eq!(results[1].domain, "b.com");
        assert_eq!(results[2].domain, "c.com");
    }

    #[test]
    fn collect_suggestions_caps_at_max() {
        let items: Vec<Suggestion> = (0..20)
            .map(|i| make_suggestion(&format!("domain{i}.com")))
            .collect();
        let results = collect_suggestions(&items, MAX_SUGGESTIONS);
        assert_eq!(results.len(), MAX_SUGGESTIONS);
    }

    #[test]
    fn collect_suggestions_skips_items_without_domain() {
        let items = vec![
            Suggestion {
                domain: None,
                inventory: None,
                prices: None,
            },
            make_suggestion("valid.com"),
            Suggestion {
                domain: None,
                inventory: None,
                prices: None,
            },
        ];
        let results = collect_suggestions(&items, 10);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].domain, "valid.com");
    }

    #[test]
    fn collect_suggestions_empty_input() {
        let results = collect_suggestions(&[], 10);
        assert!(results.is_empty());
    }
}
