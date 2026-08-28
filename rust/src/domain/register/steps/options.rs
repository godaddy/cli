//! Step 2: Registration options — period, privacy, auto-renew, custom
//! nameservers.

use cli_engine::{CliCoreError, Result};
use console::style;
use dialoguer::{Confirm, Input, Select};
use domains_client::types;

use crate::domain::common::{
    api_error, clamp_registration_periods, is_period_limit_error, make_client_with_cred,
    parse_max_registration_period, period_label, periods_from_prices, validate_domain_name,
};
use crate::retry::with_retry;

use super::super::wizard::{StepContext, StepResult, WizardState};

pub(crate) async fn run(state: &mut WizardState, ctx: &StepContext) -> Result<StepResult> {
    eprintln!(
        "\n  {} Configuring registration for {}",
        style("⚙").bold(),
        style(state.domain.as_deref().unwrap_or("unknown")).cyan()
    );

    ensure_available_periods(state, ctx).await?;
    refine_periods_with_quote_limit(state, ctx).await?;

    let period_labels: Vec<String> = state
        .available_periods
        .iter()
        .map(|p| period_label(*p))
        .collect();

    let default_idx = state
        .available_periods
        .iter()
        .position(|&p| p == state.period)
        .unwrap_or(0);

    let period_idx = Select::new()
        .with_prompt("Registration period")
        .items(&period_labels)
        .default(default_idx)
        .interact()
        .map_err(|e| CliCoreError::message(format!("prompt cancelled: {e}")))?;
    state.period = state.available_periods[period_idx];

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

    let back_label = if ctx.wizard_start_at > 0 {
        "↩ Go back to change domain"
    } else {
        "↩ Go back to discovery"
    };
    let choices = vec!["Continue", back_label];
    let selection = Select::new()
        .items(&choices)
        .default(0)
        .interact()
        .map_err(|e| CliCoreError::message(format!("prompt cancelled: {e}")))?;

    if selection == 1 {
        return Ok(StepResult::Back);
    }

    Ok(StepResult::Continue)
}

/// Load priced registration periods for the selected domain when discovery or
/// a bridge entry did not already populate them.
async fn ensure_available_periods(state: &mut WizardState, ctx: &StepContext) -> Result<()> {
    if !state.available_periods.is_empty() {
        return Ok(());
    }

    let domain = state
        .domain
        .as_ref()
        .ok_or_else(|| CliCoreError::message("no domain selected"))?;
    let domain = validate_domain_name(domain)?;

    let client = make_client_with_cred(&ctx.env, &ctx.credential)?;
    let debug = ctx.debug;

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

    if !availability.available.unwrap_or(false) {
        return Err(CliCoreError::message(format!(
            "{domain} is no longer available for registration"
        )));
    }

    state.available_periods = periods_from_prices(&availability.prices.unwrap_or_default());
    if state.available_periods.is_empty() {
        state.available_periods = vec![1];
    }

    Ok(())
}

/// Availability pricing is indicative; probe the quote endpoint with the longest
/// offered period to learn the TLD's authoritative maximum.
async fn refine_periods_with_quote_limit(state: &mut WizardState, ctx: &StepContext) -> Result<()> {
    let Some(max_offered) = state.available_periods.last().copied() else {
        return Ok(());
    };
    if max_offered <= 1 {
        return Ok(());
    }

    let domain = state
        .domain
        .as_ref()
        .ok_or_else(|| CliCoreError::message("no domain selected"))?
        .clone();
    let domain = validate_domain_name(&domain)?;

    let period_nz = std::num::NonZeroU64::new(max_offered)
        .ok_or_else(|| CliCoreError::message("invalid registration period"))?;

    let client = make_client_with_cred(&ctx.env, &ctx.credential)?;
    let profile = types::InlineRegistrationProfile {
        auto_renew: Some(state.auto_renew),
        contacts: None,
        name_servers: None,
        privacy: Some(state.privacy),
    };

    match client
        .quote_domain_registration()
        .body(types::QuoteDomainRegistrationBody {
            domain,
            period: period_nz,
            profile: Some(profile),
            profile_id: None,
        })
        .send()
        .await
    {
        Ok(_) => Ok(()),
        Err(domains_client::Error::UnexpectedResponse(resp)) => {
            let body = resp.text().await.unwrap_or_default();
            if is_period_limit_error(&body)
                && let Some(max_years) = parse_max_registration_period(&body)
            {
                let before = state.available_periods.len();
                clamp_registration_periods(
                    &mut state.available_periods,
                    &mut state.period,
                    max_years,
                );
                if state.available_periods.len() < before {
                    eprintln!(
                        "  {} This TLD supports up to {} years; longer indicative \
                         prices from availability were removed.",
                        style("ℹ").cyan().bold(),
                        max_years
                    );
                }
            }
            Ok(())
        }
        Err(e) => Err(api_error("domain quote", ctx.debug, e).await),
    }
}

fn prompt_nameservers() -> Result<Vec<String>> {
    let mut nameservers = Vec::new();
    eprintln!("  Enter nameservers (empty line to finish, min 2):");
    loop {
        let ns: String = Input::new()
            .with_prompt(format!("  Enter NS {}", nameservers.len() + 1))
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

        match validate_domain_name(&ns) {
            Ok(valid) => nameservers.push(valid),
            Err(e) => {
                eprintln!("  Invalid nameserver: {e}");
                continue;
            }
        }
    }
    Ok(nameservers)
}

#[cfg(test)]
mod tests {
    #[test]
    fn default_period_index_falls_back_when_current_period_unavailable() {
        let periods = [1_u64, 2, 3];
        let current = 5_u64;
        let default_idx = periods.iter().position(|&p| p == current).unwrap_or(0);
        assert_eq!(default_idx, 0);
        assert_eq!(periods[default_idx], 1);
    }
}
