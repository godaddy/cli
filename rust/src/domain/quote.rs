//! `gddy domain quote` — price a registration, lock a quote, cache it (v3).

use cli_engine::{
    CliCoreError, CommandResult, CommandSpec, NextAction, NextActionParam, Result,
    RuntimeCommandSpec, Tier,
};
use serde_json::json;

use domains_client::types;

use super::common::{api_error, format_money, make_client, string_list};
use crate::output_schema::output_schema;
use crate::scopes::DOMAINS_READ;
use crate::{contacts, quote_cache};

// Mirrors what `quote_to_json` emits: `domain`/`available` are always present;
// the rest appear only when the API supplies them (available + priced quotes).
output_schema!(DomainQuoteResult {
    "domain": "string";
    "available": "bool";
    "price": "string", optional;
    "currency": "string", optional;
    "renewalPrice": "string", optional;
    "period": "number", optional;
    "quoteToken": "string", optional;
    "expiresAt": "string", optional;
    "irreversible": "bool", optional;
    "agreements": "string", optional;
    "requiredAgreements": "[]object", optional;
    "resolved": "object", optional;
});

/// Build the inline registration profile (contacts + preferences) sent with a
/// quote. Always returns a profile: `auto_renew` and `privacy` are always set
/// (from the flags/defaults), while `contacts` and `name_servers` are populated
/// only when configured. When any role is set in `contacts.toml` the registrant
/// must be present, since v3's `Contacts` requires it — that's the one error case.
fn build_profile(
    privacy: bool,
    renew_auto: bool,
    name_servers: &[String],
) -> Result<types::InlineRegistrationProfile> {
    let file = contacts::load().map_err(|e| CliCoreError::message(e.to_string()))?;
    let to_api = |role| file.to_api(role).map_err(CliCoreError::message);
    let registrant = to_api(contacts::Role::Registrant)?;
    let admin = to_api(contacts::Role::Admin)?;
    let billing = to_api(contacts::Role::Billing)?;
    let tech = to_api(contacts::Role::Tech)?;

    let any_non_registrant = admin.is_some() || billing.is_some() || tech.is_some();
    let contacts_obj = match registrant {
        Some(registrant) => Some(types::Contacts {
            registrant,
            admin,
            billing,
            tech,
        }),
        None if any_non_registrant => {
            return Err(CliCoreError::message(
                "contacts.toml defines a non-registrant contact but no [registrant]; v3 requires a \
                 registrant contact when any contact is supplied. Add a [registrant] section or \
                 remove the others to use your account defaults.",
            ));
        }
        None => None,
    };

    let name_servers = (!name_servers.is_empty()).then(|| {
        types::NameServers(
            name_servers
                .iter()
                .map(|h| types::NameserverHostname(h.clone()))
                .collect(),
        )
    });

    Ok(types::InlineRegistrationProfile {
        auto_renew: Some(renew_auto),
        contacts: contacts_obj,
        name_servers,
        privacy: Some(privacy),
    })
}

/// Render a required agreement as a human line ("Title (url)" / "Title"), for the
/// quote's default view and the purchase `--agree` review prompt.
fn agreement_line(a: &types::Agreement) -> String {
    let title = a.title.as_deref().unwrap_or("(untitled agreement)");
    match a.url.as_deref() {
        Some(url) => format!("{title} ({url})"),
        None => title.to_owned(),
    }
}

/// Build the JSON view of a v3 quote. The default (human) view surfaces the
/// locked price, the token + its expiry, a `resolved` settings summary, and an
/// `agreements` scalar joining the required-agreement titles — so the terms are
/// visible without `--output json`. The full structured `requiredAgreements`
/// array is kept for scripting.
fn quote_to_json(quote: &types::RegistrationQuote) -> serde_json::Value {
    let mut out = json!({
        "domain": quote.domain,
        "available": quote.available,
    });
    if let Some(price) = quote.price.as_ref()
        && let Some(p) = format_money(price)
    {
        out["price"] = json!(p);
        out["currency"] = json!(price.currency_code.as_ref().map(|c| c.to_string()));
    }
    if let Some(renewal) = quote.renewal_price.as_ref().and_then(format_money) {
        out["renewalPrice"] = json!(renewal);
    }
    if let Some(period) = quote.period {
        out["period"] = json!(period.get());
    }
    if let Some(token) = quote.quote_token.as_ref() {
        out["quoteToken"] = json!(token.to_string());
    }
    if let Some(expires) = quote.expires_at.as_ref() {
        out["expiresAt"] = json!(expires.to_string());
    }
    if let Some(irreversible) = quote.irreversible {
        out["irreversible"] = json!(irreversible);
    }
    // The effective settings the registration would apply (contacts source,
    // privacy, auto-renew, nameservers) — so the user reviews what they're buying.
    if let Some(resolved) = quote.resolved.as_ref()
        && let Ok(value) = serde_json::to_value(resolved)
    {
        out["resolved"] = value;
    }
    if let Some(agreements) = quote.required_agreements.as_ref() {
        // Scalar summary for the default table view (nested arrays don't project).
        out["agreements"] = json!(
            agreements
                .iter()
                .map(agreement_line)
                .collect::<Vec<_>>()
                .join("; ")
        );
        // Full structure for `--output json`.
        out["requiredAgreements"] = json!(
            agreements
                .iter()
                .map(|a| json!({
                    "agreementType": a.agreement_type.as_ref().map(|t| t.to_string()),
                    "title": a.title,
                    "url": a.url,
                }))
                .collect::<Vec<_>>()
        );
    }
    out
}

/// Split a quote's required agreements into (types, human-title lines) for the
/// quote cache — the types are echoed into `consent.agreementTypes` at purchase,
/// the titles drive the `--agree` review prompt.
fn agreement_types_and_titles(agreements: &[types::Agreement]) -> (Vec<String>, Vec<String>) {
    let types = agreements
        .iter()
        .filter_map(|a| a.agreement_type.as_ref().map(|t| t.to_string()))
        .collect();
    let titles = agreements.iter().map(agreement_line).collect();
    (types, titles)
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_with_context(
        CommandSpec::new("quote", "Price a domain registration and lock a quote")
            .with_long(
                "Quote a single-domain registration: returns the locked price, the \
                 legal agreements you must accept, the contact/preference settings that \
                 would apply, and a single-use quote token (valid ~10 minutes). Quoting \
                 is free and changes nothing.\n\
                 \n\
                 The quote — including the registration settings you pass here \
                 (--period/--privacy/--no-renew/--nameserver and your contacts.toml) — is \
                 saved locally so `gddy domain purchase --quote-token <token>` buys the \
                 exact thing you reviewed, at the price you were quoted.",
            )
            .with_system("domain")
            .with_tier(Tier::Read)
            .with_default_fields(
                "domain,available,price,currency,period,quoteToken,expiresAt,agreements",
            )
            .with_output_schema::<DomainQuoteResult>()
            .with_scopes(&[DOMAINS_READ])
            .with_arg(
                clap::Arg::new("domain")
                    .value_name("DOMAIN")
                    .required(true)
                    .help("Domain to quote, e.g. example.com"),
            )
            .with_arg(
                clap::Arg::new("period")
                    .long("period")
                    .value_name("YEARS")
                    .value_parser(clap::value_parser!(u64).range(1..=10))
                    .default_value("1")
                    .help("Registration length in years (1-10)"),
            )
            .with_arg(
                clap::Arg::new("privacy")
                    .long("privacy")
                    .action(clap::ArgAction::SetTrue)
                    .help("Add privacy protection to the registration"),
            )
            .with_arg(
                clap::Arg::new("no-renew")
                    .long("no-renew")
                    .action(clap::ArgAction::SetTrue)
                    .help("Disable auto-renewal (auto-renew is on by default)"),
            )
            .with_arg(
                clap::Arg::new("nameserver")
                    .long("nameserver")
                    .value_name("HOST")
                    .action(clap::ArgAction::Append)
                    .help("Custom nameserver (repeatable); omit to use GoDaddy defaults"),
            ),
        |ctx| async move {
            let domain = ctx
                .args
                .get("domain")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_owned();
            let period = ctx.args.get("period").and_then(|v| v.as_u64()).unwrap_or(1);
            let privacy = ctx
                .args
                .get("privacy")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let renew_auto = !ctx
                .args
                .get("no-renew")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let name_servers = string_list(&ctx, "nameserver");
            let debug = !ctx.middleware.debug.is_empty();
            let period_nz =
                std::num::NonZeroU64::new(period).expect("clap value_parser enforces period >= 1");

            // Resolve auth first (fail closed before reading local config), then
            // build the profile. The token is bound to a hash of this quoted
            // request, so what's quoted here is what `purchase` later registers.
            let client = make_client(&ctx).await?;
            let profile = build_profile(privacy, renew_auto, &name_servers)?;
            // Cache the exact profile we quote with: the token binds a hash of the
            // domain/price/profile, so `purchase` must re-send this verbatim or the
            // register call fails with QUOTE_MISMATCH.
            let profile_json = serde_json::to_value(&profile).ok();

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
                Err(e) => return Err(api_error("quoting registration", debug, e).await),
            };

            let view = quote_to_json(&quote);

            // Persist the quote so `purchase --quote-token` can reproduce this
            // exact request (only possible when it's available and got a token).
            let mut next_actions = Vec::new();
            if quote.available.unwrap_or(false)
                && let Some(token) = quote.quote_token.as_ref()
            {
                let token = token.to_string();
                let agreements = quote.required_agreements.clone().unwrap_or_default();
                let (agreement_types, agreement_titles) = agreement_types_and_titles(&agreements);
                let cached = quote_cache::CachedQuote {
                    domain: quote.domain.clone().unwrap_or_else(|| domain.clone()),
                    period: quote.period.map_or(period, |p| p.get()),
                    agreement_types,
                    agreement_titles,
                    price: view
                        .get("price")
                        .and_then(|v| v.as_str())
                        .map(str::to_owned),
                    currency: view
                        .get("currency")
                        .and_then(|v| v.as_str())
                        .map(str::to_owned),
                    expires_at: quote.expires_at.as_ref().map(|e| e.to_string()),
                    profile: profile_json,
                };
                if let Err(e) = quote_cache::save(&token, cached) {
                    // Non-fatal: the quote is still shown, but purchase won't
                    // find it. Warn so the user knows to re-quote on this host.
                    tracing::warn!(error = %e, "could not cache the quote for purchase");
                }
                next_actions.push(
                    NextAction::new(
                        "domain purchase --quote-token <token> --agree --confirm",
                        "Register at the quoted price (within ~10 minutes)",
                    )
                    .with_param("quote-token", NextActionParam::required()),
                );
            } else {
                // Not available (or no token was issued): point at discovery, the
                // same next step `domain available` offers for a taken name.
                next_actions.push(
                    NextAction::new("domain suggest <query>", "Find an available alternative")
                        .with_param("query", NextActionParam::required()),
                );
            }

            Ok(CommandResult::new(view).with_next_actions(next_actions))
        },
    )
}
