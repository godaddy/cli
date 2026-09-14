//! Step 4: Review — fetch a quote, display agreements, show order summary, and
//! request confirmation before executing. Also handles 402 Payment Required
//! by offering to open the browser for payment method setup.

use cli_engine::{CliCoreError, Result};
use console::{Alignment, pad_str, style};
use dialoguer::{Confirm, Select};

use domains_client::types;

use crate::contacts::Role;
use crate::domain::common::{
    api_error, clamp_registration_periods, format_api_error, format_money, is_period_limit_error,
    make_client_with_cred, parse_max_registration_period, period_label, validate_nameserver_hosts,
};
use crate::environments;
use crate::quote_cache;

use crate::retry::with_retry;

use super::super::wizard::{ContactsChoice, StepContext, StepResult, WizardState};

/// Inner content width for the order-summary box (between the `│ ` and ` │`).
const SUMMARY_INNER_WIDTH: usize = 38;

/// One padded line of the order-summary box. Uses `console::pad_str` so ANSI
/// color codes and wide glyphs (emoji) don't shift the right border.
fn summary_line(content: &str) -> String {
    format!(
        "  │ {}│",
        pad_str(content, SUMMARY_INNER_WIDTH, Alignment::Left, None)
    )
}

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

    // Resolve contacts from the wizard state.
    let contacts = build_contacts_for_profile(&state.contacts)?;

    let profile = types::InlineRegistrationProfile {
        auto_renew: Some(state.auto_renew),
        contacts,
        name_servers,
        privacy: Some(state.privacy),
    };
    let profile_json = serde_json::to_value(&profile).map_err(|e| {
        CliCoreError::message(format!("could not serialize registration profile: {e}"))
    })?;

    let quote_body = types::QuoteDomainRegistrationBody {
        domain: domain.clone(),
        period: period_nz,
        profile: Some(profile),
        profile_id: None,
    };
    let quote = match with_retry("quote", 3, || {
        let c = &client;
        let b = quote_body.clone();
        async move { c.quote_domain_registration().body(b).send().await }
    })
    .await
    {
        Ok(r) => r.into_inner(),
        Err(e) => match e {
            domains_client::Error::UnexpectedResponse(resp) => {
                let status = resp.status();
                let request_id = resp
                    .headers()
                    .get("x-request-id")
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_owned);
                let body = resp.text().await.unwrap_or_default();
                if is_period_limit_error(&body)
                    && let Some(max_years) = parse_max_registration_period(&body)
                {
                    let rejected = state.period;
                    clamp_registration_periods(
                        &mut state.available_periods,
                        &mut state.period,
                        max_years,
                    );
                    state.period_prices.retain(|years, _| *years <= max_years);
                    eprintln!(
                        "\n  {} Period {} is not supported for this domain (maximum is {}). \
                         Go back to Options to choose a different length.",
                        style("⚠").yellow().bold(),
                        rejected,
                        max_years
                    );
                    return Ok(StepResult::Back);
                }
                let err = CliCoreError::message(format_api_error(
                    "domain quote",
                    status.as_u16(),
                    &status.to_string(),
                    &body,
                    request_id.as_deref(),
                    debug,
                ));
                if is_payment_error(&err) {
                    return handle_payment_required(ctx).await;
                }
                return Err(err);
            }
            other => {
                let err = api_error("domain quote", debug, other).await;
                if is_payment_error(&err) {
                    return handle_payment_required(ctx).await;
                }
                return Err(err);
            }
        },
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

    // Display order summary. Pad by visible width (not byte length) so
    // styled/colored fields keep the right border aligned.
    let w = SUMMARY_INNER_WIDTH + 1; // border fill between ┌ and ┐
    eprintln!("\n  ┌{}┐", "─".repeat(w));
    eprintln!(
        "{}",
        summary_line(&format!("{}", style("Order Summary").bold()))
    );
    eprintln!("  ├{}┤", "─".repeat(w));
    eprintln!(
        "{}",
        summary_line(&format!("Domain:     {}", style(&domain).cyan().bold()))
    );
    eprintln!(
        "{}",
        summary_line(&format!("Period:     {}", period_label(state.period)))
    );
    if let Some(p) = &price_str {
        eprintln!(
            "{}",
            summary_line(&format!(
                "Total due:  {} {}",
                style(p).green().bold(),
                currency
            ))
        );
    }
    if let Some(r) = &renewal_str {
        eprintln!(
            "{}",
            summary_line(&format!(
                "Renewal price ({}): {r} {currency}",
                period_label(state.period)
            ))
        );
    }
    eprintln!(
        "{}",
        summary_line(&format!(
            "Privacy:    {}",
            if state.privacy { "Yes" } else { "No" }
        ))
    );
    eprintln!(
        "{}",
        summary_line(&format!(
            "Auto-renew: {}",
            if state.auto_renew { "Yes" } else { "No" }
        ))
    );
    if !state.nameservers.is_empty() {
        eprintln!(
            "{}",
            summary_line(&format!("Nameservers: {}", state.nameservers.join(", ")))
        );
    }
    eprintln!("  └{}┘", "─".repeat(w));

    // Show agreements.
    if !agreement_titles.is_empty() {
        eprintln!("\n  Legal agreements:");
        for title in &agreement_titles {
            eprintln!("    • {title}");
        }
    }

    // Confirm.
    let choices = vec![
        format!(
            "✓ I agree to the terms above and authorize a charge of {} {}",
            price_str.as_deref().unwrap_or("the quoted price"),
            currency
        ),
        "↩ Go back and change options".to_string(),
        "✗ Cancel — do not purchase".to_string(),
    ];
    let selection = Select::new()
        .with_prompt("By proceeding you accept the legal agreements listed above")
        .items(&choices)
        .default(0)
        .interact()
        .map_err(|e| CliCoreError::message(format!("prompt cancelled: {e}")))?;

    match selection {
        1 => return Ok(StepResult::Back),
        2 => return Ok(StepResult::Cancel),
        _ => {} // 0 = proceed
    }

    // Cache the quote for the execute step.
    let quote_token = quote
        .quote_token
        .as_ref()
        .ok_or_else(|| CliCoreError::message("the quote returned no token (domain unavailable?)"))?
        .to_string();
    let idempotency_key = uuid::Uuid::new_v4().to_string();
    let fees_json = match quote.fees.as_ref().filter(|f| !f.is_empty()) {
        Some(fees) => Some(serde_json::to_value(fees).map_err(|e| {
            CliCoreError::message(format!(
                "could not serialize the quote's fees for the quote cache: {e}"
            ))
        })?),
        None => None,
    };
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

/// Non-interactive variant: fetches a quote and caches it without prompting
/// for confirmation. Callers must have already validated `--agree` and
/// `--confirm` flags before reaching this point.
pub(crate) async fn run_non_interactive(
    state: &mut WizardState,
    ctx: &StepContext,
) -> Result<StepResult> {
    let domain = state
        .domain
        .as_ref()
        .ok_or_else(|| CliCoreError::message("no domain selected"))?
        .clone();

    let client = make_client_with_cred(&ctx.env, &ctx.credential)?;
    let debug = ctx.debug;

    let period_nz = std::num::NonZeroU64::new(state.period)
        .ok_or_else(|| CliCoreError::message("invalid registration period"))?;

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

    let contacts = build_contacts_for_profile(&state.contacts)?;

    let profile = types::InlineRegistrationProfile {
        auto_renew: Some(state.auto_renew),
        contacts,
        name_servers,
        privacy: Some(state.privacy),
    };
    let profile_json = serde_json::to_value(&profile).map_err(|e| {
        CliCoreError::message(format!("could not serialize registration profile: {e}"))
    })?;

    let quote_body = types::QuoteDomainRegistrationBody {
        domain: domain.clone(),
        period: period_nz,
        profile: Some(profile),
        profile_id: None,
    };
    let quote = match with_retry("quote", 3, || {
        let c = &client;
        let b = quote_body.clone();
        async move { c.quote_domain_registration().body(b).send().await }
    })
    .await
    {
        Ok(r) => r.into_inner(),
        Err(e) => {
            let err = api_error("domain quote", debug, e).await;
            if is_payment_error(&err) {
                return Err(CliCoreError::message(
                    "no usable payment method on file; add one at \
                     https://account.godaddy.com/payment-methods before retrying",
                ));
            }
            return Err(err);
        }
    };

    let price_str = quote.price.as_ref().and_then(format_money);
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

    let quote_token = quote
        .quote_token
        .as_ref()
        .ok_or_else(|| CliCoreError::message("the quote returned no token (domain unavailable?)"))?
        .to_string();
    let idempotency_key = uuid::Uuid::new_v4().to_string();
    let fees_json = match quote.fees.as_ref().filter(|f| !f.is_empty()) {
        Some(fees) => Some(serde_json::to_value(fees).map_err(|e| {
            CliCoreError::message(format!(
                "could not serialize the quote's fees for the quote cache: {e}"
            ))
        })?),
        None => None,
    };
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

/// Convert the wizard's contacts choice into the API's `Contacts` struct.
fn build_contacts_for_profile(choice: &ContactsChoice) -> Result<Option<types::Contacts>> {
    let file = match choice {
        ContactsChoice::AccountDefault => return Ok(None),
        ContactsChoice::FromFile(f) | ContactsChoice::Manual(f) => f,
    };

    let to_api = |role| file.to_api(role).map_err(CliCoreError::message);
    let registrant = to_api(Role::Registrant)?;
    let admin = to_api(Role::Admin)?;
    let billing = to_api(Role::Billing)?;
    let tech = to_api(Role::Tech)?;

    let any_non_registrant = admin.is_some() || billing.is_some() || tech.is_some();
    match registrant {
        Some(registrant) => Ok(Some(types::Contacts {
            registrant,
            admin,
            billing,
            tech,
        })),
        None if any_non_registrant => Err(CliCoreError::message(
            "contacts define a non-registrant contact but no registrant; the API requires a \
             registrant when any contact is supplied",
        )),
        None => Ok(None),
    }
}

/// Check if a CLI error is a 402 Payment Required error.
fn is_payment_error(err: &CliCoreError) -> bool {
    let msg = err.to_string();
    msg.contains("402") || msg.contains("INVALID_PAYMENT_INFO") || msg.contains("payment")
}

/// Handle 402: inform the user and offer to open the payment methods page.
async fn handle_payment_required(ctx: &StepContext) -> Result<StepResult> {
    eprintln!(
        "\n  {} No usable payment method found on your account.",
        style("⚠").yellow().bold()
    );
    eprintln!("  A credit card or Good-as-Gold balance is required for domain purchases.");

    let open_browser = Confirm::new()
        .with_prompt("Open the GoDaddy payment methods page in your browser?")
        .default(true)
        .interact()
        .map_err(|e| CliCoreError::message(format!("prompt cancelled: {e}")))?;

    if open_browser {
        let env = environments::resolve(&ctx.env)?;
        let url = format!("{}/payment-methods/add-payment?plid=1", env.account_url);
        if open::that(&url).is_err() {
            eprintln!(
                "  Could not open browser. Visit: {}",
                style(&url).underlined()
            );
        } else {
            eprintln!("  {} Browser opened.", style("✓").green().bold());
        }
    }

    eprintln!("\n  After adding a payment method, select '↩ Go back' to retry.");

    let retry_choices = vec!["↩ Go back and retry the quote", "✗ Cancel registration"];
    let selection = Select::new()
        .with_prompt("What would you like to do?")
        .items(&retry_choices)
        .default(0)
        .interact()
        .map_err(|e| CliCoreError::message(format!("prompt cancelled: {e}")))?;

    match selection {
        0 => Ok(StepResult::Back),
        _ => Ok(StepResult::Cancel),
    }
}

#[cfg(test)]
mod tests {
    use super::{SUMMARY_INNER_WIDTH, summary_line};
    use console::{measure_text_width, style};

    #[test]
    fn summary_line_keeps_right_border_aligned_with_ansi_styles() {
        let plain = summary_line("Period:     2 years");
        let styled = summary_line(&format!(
            "Domain:     {}",
            style("iguanahats.shop").cyan().bold()
        ));
        let priced = summary_line(&format!(
            "Total due:  {} USD",
            style("60.98").green().bold()
        ));
        let renewal = summary_line("Renewal price (3 years): 179.97 USD");

        // Visible width (ANSI stripped) must match across plain and styled
        // rows so the box's right `│` lines up in a real terminal.
        let widths = [
            measure_text_width(&plain),
            measure_text_width(&styled),
            measure_text_width(&priced),
            measure_text_width(&renewal),
        ];
        assert!(
            widths.iter().all(|&w| w == widths[0]),
            "visible widths drifted: {widths:?}\nplain={plain:?}\nstyled={styled:?}\npriced={priced:?}\nrenewal={renewal:?}"
        );
        // "  │ " (4) + inner + "│" (1)
        assert_eq!(widths[0], 4 + SUMMARY_INNER_WIDTH + 1);
        assert!(plain.ends_with('│'));
        assert!(styled.ends_with('│'));
        assert!(priced.ends_with('│'));
        assert!(renewal.ends_with('│'));
    }
}
