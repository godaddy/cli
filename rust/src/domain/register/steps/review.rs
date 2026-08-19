//! Step 3: Review — fetch a quote, display agreements, show order summary, and
//! request confirmation before executing.

use cli_engine::{CliCoreError, Result};
use console::style;
use dialoguer::Confirm;

use domains_client::types;

use crate::domain::common::{
    api_error, format_money, make_client_with_cred, period_label, validate_nameserver_hosts,
};
use crate::quote_cache;

use super::super::wizard::{StepContext, StepResult, WizardState};

pub(crate) async fn run(state: &mut WizardState, ctx: &StepContext) -> Result<StepResult> {
    let domain = state
        .domain
        .as_ref()
        .ok_or_else(|| CliCoreError::message("no domain selected"))?
        .clone();

    eprintln!(
        "\n  {} Fetching quote for {}...",
        style("$").bold(),
        style(&domain).cyan()
    );

    let client = make_client_with_cred(&ctx.env, &ctx.credential)?;
    let debug = ctx.debug;

    let period_nz = std::num::NonZeroU64::new(state.period)
        .ok_or_else(|| CliCoreError::message("invalid registration period"))?;

    // Build the registration profile for the quote.
    let name_servers = if state.nameservers.is_empty() {
        None
    } else {
        let validated = validate_nameserver_hosts(state.nameservers.clone())?;
        Some(types::NameServers(
            validated
                .iter()
                .map(|h| types::NameserverHostname(h.clone()))
                .collect(),
        ))
    };

    let profile = types::InlineRegistrationProfile {
        auto_renew: Some(state.auto_renew),
        contacts: None,
        name_servers,
        privacy: Some(state.privacy),
    };
    let profile_json = serde_json::to_value(&profile).map_err(|e| {
        CliCoreError::message(format!("could not serialize registration profile: {e}"))
    })?;

    let quote = match client
        .quote_domain_registration()
        .body(types::QuoteDomainRegistrationBody {
            domain: domain.clone(),
            period: period_nz,
            profile: Some(profile),
            profile_id: None,
        })
        .send()
        .await
    {
        Ok(r) => r.into_inner(),
        Err(e) => return Err(api_error("domain quote", debug, e).await),
    };

    // Extract pricing and agreement info.
    let price_str = quote.price.as_ref().and_then(format_money);
    let renewal_str = quote.renewal_price.as_ref().and_then(format_money);
    let currency = quote
        .price
        .as_ref()
        .and_then(|p| p.currency_code.as_ref())
        .map(|c| c.0.clone())
        .unwrap_or_default();

    let agreements = quote.required_agreements.clone().unwrap_or_default();
    let agreement_titles: Vec<String> = agreements
        .iter()
        .map(|a| {
            let title = a.title.as_deref().unwrap_or("(untitled)");
            match a.url.as_deref() {
                Some(url) => format!("{title} ({url})"),
                None => title.to_owned(),
            }
        })
        .collect();
    let agreement_types: Vec<String> = agreements
        .iter()
        .filter_map(|a| a.agreement_type.as_ref().map(|t| t.to_string()))
        .collect();

    // Display order summary.
    eprintln!("\n  ┌─────────────────────────────────────");
    eprintln!("  │ {} Order Summary", style("📋").bold());
    eprintln!("  ├─────────────────────────────────────");
    eprintln!("  │ Domain:   {}", style(&domain).cyan().bold());
    eprintln!("  │ Period:   {}", period_label(state.period));
    if let Some(p) = &price_str {
        eprintln!("  │ Price:    {} {}", style(p).green().bold(), currency);
    }
    if let Some(r) = &renewal_str {
        eprintln!("  │ Renewal:  {} {}/yr", r, currency);
    }
    eprintln!("  │ Privacy:  {}", if state.privacy { "Yes" } else { "No" });
    eprintln!(
        "  │ Auto-renew: {}",
        if state.auto_renew { "Yes" } else { "No" }
    );
    if !state.nameservers.is_empty() {
        eprintln!("  │ Nameservers: {}", state.nameservers.join(", "));
    }
    eprintln!("  └─────────────────────────────────────");

    // Show agreements.
    if !agreement_titles.is_empty() {
        eprintln!("\n  Legal agreements:");
        for title in &agreement_titles {
            eprintln!("    • {title}");
        }
    }

    // Confirm.
    let confirmed = Confirm::new()
        .with_prompt(format!(
            "Proceed with registration? This will charge {} {} to your account",
            price_str.as_deref().unwrap_or("the quoted price"),
            currency
        ))
        .default(false)
        .interact()
        .map_err(|e| CliCoreError::message(format!("prompt cancelled: {e}")))?;

    if !confirmed {
        return Ok(StepResult::Cancel);
    }

    // Cache the quote for the execute step.
    let quote_token = quote
        .quote_token
        .as_ref()
        .ok_or_else(|| CliCoreError::message("the quote returned no token (domain unavailable?)"))?
        .to_string();
    let idempotency_key = uuid::Uuid::new_v4().to_string();
    let fees_json = quote
        .fees
        .as_ref()
        .and_then(|f| serde_json::to_value(f).ok());
    quote_cache::save(
        &quote_token,
        quote_cache::CachedQuote {
            domain: quote.domain.clone().unwrap_or_else(|| domain.clone()),
            period: quote.period.map_or(state.period, |p| p.get()),
            price: price_str.clone(),
            currency: Some(currency.clone()),
            agreement_titles: agreement_titles.clone(),
            agreement_types: agreement_types.clone(),
            profile: Some(profile_json),
            idempotency_key: Some(idempotency_key),
            expires_at: quote.expires_at.as_ref().map(|e| e.to_string()),
            fees: fees_json,
        },
    )?;

    state.quote_token = Some(quote_token);
    state.price = price_str;
    state.currency = Some(currency);
    state.agreement_titles = agreement_titles;
    state.agreement_types = agreement_types;

    Ok(StepResult::Continue)
}
