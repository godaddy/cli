//! `gddy domain quote` — price a registration, lock a quote, cache it (v3).

use cli_engine::{
    Alignment, CliCoreError, CommandResult, CommandSpec, NextActionParam, Result,
    RuntimeCommandSpec, TableColumn, Tier,
};
use serde_json::json;

use domains_client::types;

use super::common::{
    fetch_with_domain_retry, format_money, period_label, resolve_domain_name,
    resolve_nameserver_hosts,
};
use crate::next_action::next_action;
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
    "periodLabel": "string", optional;
    "quoteToken": "string", optional;
    "expiresAt": "string", optional;
    "irreversible": "bool", optional;
    "agreements": "string", optional;
    "requiredAgreements": "[]object", optional;
    "resolved": "object", optional;
    // Present only for premium (Afternic) domains: `inventory` is `PREMIUM` and
    // `fees` carries the one-time acquisition surcharge to acknowledge at purchase.
    "inventory": "string", optional;
    "fees": "[]object", optional;
});

/// Render a quote's `fees` array (e.g. a premium domain's one-time acquisition
/// surcharge) as `{type, amount, currency}` objects for display — mirrors how
/// `quote_to_json` renders prices via [`format_money`]. `None` (rather than an
/// empty array) when the quote carried no fees, so the field is omitted from
/// output entirely instead of showing an empty list.
fn fees_to_json(fees: &[types::Fee]) -> Option<serde_json::Value> {
    if fees.is_empty() {
        return None;
    }
    Some(json!(
        fees.iter()
            .map(|f| {
                let mut out = json!({});
                if let Some(t) = f.type_.as_ref() {
                    out["type"] = json!(t.to_string());
                }
                if let Some(amount) = f.fee.as_ref().and_then(format_money) {
                    out["amount"] = json!(amount);
                    if let Some(code) = f.fee.as_ref().and_then(|m| m.currency_code.as_ref()) {
                        out["currency"] = json!(code.to_string());
                    }
                }
                out
            })
            .collect::<Vec<_>>()
    ))
}

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
fn quote_to_json(quote: &types::RegistrationQuote, request_domain: &str) -> serde_json::Value {
    // Emit the required identity fields concretely (never JSON null): domain
    // falls back to the known request domain, and a missing `available` reads as
    // false (which is how callers already treat it).
    let mut out = json!({
        "domain": quote.domain.clone().unwrap_or_else(|| request_domain.to_owned()),
        "available": quote.available.unwrap_or(false),
    });
    if let Some(price) = quote.price.as_ref()
        && let Some(p) = format_money(price)
    {
        out["price"] = json!(p);
        // Only emit `currency` when a code is present — never a JSON null (the
        // schema declares it an optional string, and null leaks into tables).
        if let Some(code) = price.currency_code.as_ref() {
            out["currency"] = json!(code.to_string());
        }
    }
    if let Some(renewal) = quote.renewal_price.as_ref().and_then(format_money) {
        out["renewalPrice"] = json!(renewal);
    }
    if let Some(period) = quote.period {
        out["period"] = json!(period.get());
        out["periodLabel"] = json!(period_label(period.get()));
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
    if let Some(inventory) = quote.inventory.as_ref() {
        out["inventory"] = json!(inventory.to_string());
    }
    if let Some(fees) = quote.fees.as_ref().and_then(|f| fees_to_json(f)) {
        out["fees"] = fees;
    }
    // The effective settings the registration would apply (contacts source,
    // privacy, auto-renew, nameservers) — so the user reviews what they're buying.
    if let Some(resolved) = quote.resolved.as_ref()
        && let Ok(value) = serde_json::to_value(resolved)
    {
        out["resolved"] = value;
    }
    if let Some(agreements) = quote.required_agreements.as_ref() {
        // Scalar summary kept in the default fields for the common case; the
        // full array is available (nested) as `requiredAgreements` via
        // `--fields all`.
        out["agreements"] = json!(
            agreements
                .iter()
                .map(agreement_line)
                .collect::<Vec<_>>()
                .join("; ")
        );
        // Full structure for `--output json` / `--fields requiredAgreements`.
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

#[derive(Debug, Clone, clap::Args)]
struct QuoteArgs {
    /// Domain to quote, e.g. example.com.
    #[arg(value_name = "DOMAIN")]
    domain: String,

    /// Registration length in years (1-10).
    #[arg(long, value_name = "YEARS", value_parser = clap::value_parser!(u64).range(1..=10), default_value = "1")]
    period: u64,

    /// Add privacy protection to the registration.
    #[arg(long)]
    privacy: bool,

    /// Disable auto-renewal (auto-renew is on by default).
    #[arg(long = "no-renew")]
    no_renew: bool,

    /// Custom nameserver (repeatable); omit to use GoDaddy defaults.
    #[arg(long, value_name = "HOST")]
    nameserver: Vec<String>,
}

/// `requiredAgreements`/`resolved` aren't in the default fields (the
/// `agreements` scalar covers the common case), but render as nested child
/// tables/property bags instead of raw JSON dumps when selected via
/// `--fields all` or by name.
fn view_columns() -> Vec<TableColumn> {
    vec![
        TableColumn::new("domain", "Domain"),
        TableColumn::new("available", "Available"),
        TableColumn::new("price", "Price").align(Alignment::Right),
        TableColumn::new("renewalPrice", "Renewal Price").align(Alignment::Right),
        TableColumn::new("currency", "Currency"),
        TableColumn::new("periodLabel", "Period"),
        TableColumn::new("quoteToken", "Quote Token").no_truncate(true),
        TableColumn::new("expiresAt", "Expires At"),
        TableColumn::new("irreversible", "Irreversible"),
        TableColumn::new("inventory", "Inventory"),
        TableColumn::new("fees", "Fees").nested(vec![
            TableColumn::new("type", "Type"),
            TableColumn::new("amount", "Amount").align(Alignment::Right),
            TableColumn::new("currency", "Currency"),
        ]),
        TableColumn::new("agreements", "Agreements"),
        TableColumn::new("requiredAgreements", "Required Agreements").nested(vec![
            TableColumn::new("agreementType", "Type"),
            TableColumn::new("title", "Title"),
            TableColumn::new("url", "URL").no_truncate(true),
        ]),
        TableColumn::new("resolved", "Resolved Settings").nested(vec![
            TableColumn::new("contactSource", "Contact Source"),
            TableColumn::new("registrantSummary", "Registrant"),
            TableColumn::new("autoRenew", "Auto Renew"),
            TableColumn::new("privacy", "Privacy"),
            TableColumn::new("nameServers", "Name Servers"),
        ]),
    ]
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<QuoteArgs, _, _, _>(
        CommandSpec::from_args::<QuoteArgs>(
            "quote",
            "Price a domain registration and lock a quote",
        )
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
            "domain,available,price,renewalPrice,currency,period,periodLabel,quoteToken,expiresAt,agreements",
        )
        .with_output_schema::<DomainQuoteResult>()
        .with_view(view_columns())
        .with_scopes(&[DOMAINS_READ]),
        |ctx, args: QuoteArgs| async move {
            let domain = resolve_domain_name(
                &ctx,
                &args.domain,
                "Domain name to quote (e.g. example.com)",
            )?;
            let period = args.period;
            let privacy = args.privacy;
            let renew_auto = !args.no_renew;
            let name_servers = resolve_nameserver_hosts(
                &ctx,
                args.nameserver,
                "Custom nameserver host (e.g. ns1.example.com)",
            )?;
            let debug = !ctx.middleware.debug.is_empty();
            let period_nz =
                std::num::NonZeroU64::new(period).expect("clap value_parser enforces period >= 1");

            // Resolve auth first (fail closed before reading local config), then
            // build the profile. The token is bound to a hash of this quoted
            // request, so what's quoted here is what `purchase` later registers.
            let profile = build_profile(privacy, renew_auto, &name_servers)?;
            // Cache the exact profile we quote with: the token binds a hash of the
            // domain/price/profile, so `purchase` must re-send this verbatim or the
            // register call fails with QUOTE_MISMATCH. Serializing this in-memory
            // struct effectively never fails, but if it did, silently caching a
            // profile-less quote would itself cause that mismatch — so fail fast.
            let profile_json = serde_json::to_value(&profile).map_err(|e| {
                CliCoreError::message(format!(
                    "could not serialize the registration profile for the quote cache: {e}"
                ))
            })?;

            let quote = fetch_with_domain_retry(
                &ctx,
                domain.clone(),
                "Domain name to quote (e.g. example.com)",
                "quoting registration",
                debug,
                |client, domain| {
                    let profile = profile.clone();
                    async move {
                        client
                            .quote_domain_registration()
                            .body(types::QuoteDomainRegistrationBody {
                                domain,
                                period: period_nz,
                                profile: Some(profile),
                                profile_id: None,
                            })
                            .send()
                            .await
                            .map(|r| r.into_inner())
                    }
                },
            )
            .await?;

            let view = quote_to_json(&quote, &domain);

            // Persist the quote so `purchase --quote-token` can reproduce this
            // exact request (only possible when it's available and got a token).
            let mut next_actions = Vec::new();
            if quote.available.unwrap_or(false)
                && let Some(token) = quote.quote_token.as_ref()
            {
                let token = token.to_string();
                let agreements = quote.required_agreements.clone().unwrap_or_default();
                let (agreement_types, agreement_titles) = agreement_types_and_titles(&agreements);
                // Cache verbatim so `purchase` can echo it into
                // `consent.acknowledgedFees` (required for premium domains; the
                // server rejects a mismatch with `quote_mismatch`). Serializing
                // this in-memory value effectively never fails, but if it did,
                // silently caching `None` would surface as a confusing
                // `quote_mismatch` at purchase time instead of here — fail fast,
                // same as `profile_json` above.
                let fees_json = match quote.fees.as_ref().filter(|f| !f.is_empty()) {
                    Some(fees) => Some(serde_json::to_value(fees).map_err(|e| {
                        CliCoreError::message(format!(
                            "could not serialize the quote's fees for the quote cache: {e}"
                        ))
                    })?),
                    None => None,
                };
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
                    profile: Some(profile_json),
                    // Mint the register idempotency key now, so every `purchase`
                    // attempt for this token reuses it (a retry after a lost
                    // response can't double-charge).
                    idempotency_key: Some(uuid::Uuid::new_v4().to_string()),
                    fees: fees_json,
                };
                if let Err(e) = quote_cache::save(&token, cached) {
                    // Non-fatal: the quote is still shown, but purchase won't
                    // find it. Warn so the user knows to re-quote on this host.
                    tracing::warn!(error = %e, "could not cache the quote for purchase");
                }
                // If interactive, offer to purchase directly.
                let quote_price = view
                    .get("price")
                    .and_then(|v| v.as_str())
                    .map(str::to_owned);
                let quote_currency = view
                    .get("currency")
                    .and_then(|v| v.as_str())
                    .map(str::to_owned);
                match super::register::bridge::offer_registration_from_quote(
                    &ctx,
                    &domain,
                    &token,
                    quote_price,
                    quote_currency,
                    period,
                )
                .await?
                {
                    super::register::BridgeHandoff::Replace(wizard_result) => {
                        return Ok(wizard_result);
                    }
                    super::register::BridgeHandoff::ShowHostOutput => {}
                }

                next_actions.push(
                    next_action(
                        "domain purchase --quote-token <quote-token> --agree --confirm",
                        "Register at the quoted price (within ~10 minutes)",
                    )
                    .with_param("quote-token", NextActionParam::value(token)),
                );
            } else {
                // Not available (or no token was issued): point at discovery, the
                // same next step `domain available` offers for a taken name.
                // `domain suggest` accepts a seed domain, so the domain just
                // quoted is a valid query to copy/paste directly.
                let resolved_domain = quote.domain.clone().unwrap_or_else(|| domain.clone());
                next_actions.push(
                    next_action("domain suggest <query>", "Find an available alternative")
                        .with_param("query", NextActionParam::value(resolved_domain)),
                );
            }

            Ok(CommandResult::new(view).with_next_actions(next_actions))
        },
    )
}

#[cfg(test)]
mod tests {
    use super::{command, view_columns};
    use serde_json::json;

    #[test]
    fn default_fields_includes_renewal_price() {
        // Regression for GDDEVPLAT-133: `renewalPrice` was computed and present
        // in `--output json` but silently dropped from the default table view.
        // An exact field match (not a substring check) so a future field like
        // `renewalPrice1Year` can't produce a false pass here.
        let fields = command().spec.default_fields.expect("default fields set");
        assert!(fields.split(',').any(|f| f == "renewalPrice"), "{fields}");
    }

    #[test]
    fn period_renders_with_its_unit_in_the_default_table() {
        // The bare `period` number ("1") read as ambiguous; the table must show
        // `periodLabel` ("1 year") instead, while `period` itself stays numeric
        // in the payload for scripting against `--output json`.
        let quote = json!({
            "domain": "example.com",
            "available": true,
            "period": 1,
            "periodLabel": "1 year",
        });
        let envelope = cli_engine::Envelope::success(quote, "domain");
        let rendered = cli_engine::render_human_with_view(&envelope, Some(&view_columns()), "");
        assert!(rendered.contains("1 year"), "{rendered}");
    }

    /// Proves `view_columns()` renders `requiredAgreements`/`resolved` shaped
    /// like what `quote_to_json` actually emits (confirmed by inspection —
    /// `requiredAgreements` items come from a `json!` literal in
    /// `quote_to_json`, and `resolved` is `ResolvedSettings` serialized via
    /// serde, whose `#[serde(rename = ...)]` attributes were cross-checked
    /// for the field names used below) as nested blocks — a mismatch would
    /// silently drop them from `--fields all` output. Renders a hand-built
    /// envelope rather than calling `quote_to_json`, which would need a
    /// live `RegistrationQuote` built through its generated builder.
    #[test]
    fn quote_result_renders_required_agreements_and_resolved_as_nested_blocks() {
        let quote = json!({
            "domain": "example.com",
            "available": true,
            "requiredAgreements": [
                {"agreementType": "OPT_OUT_EXPLICIT", "title": "Registration Agreement", "url": "https://example.com/agreement"},
            ],
            "resolved": {
                "contactSource": "PROFILE",
                "registrantSummary": "Jane Smith / jane@example.com",
                "autoRenew": true,
                "privacy": false,
                "nameServers": ["ns01.domaincontrol.com", "ns02.domaincontrol.com"],
            },
        });
        let envelope = cli_engine::Envelope::success(quote, "domain");
        let rendered = cli_engine::render_human_with_view(&envelope, Some(&view_columns()), "");
        assert!(rendered.contains("Required Agreements:"), "{rendered}");
        assert!(rendered.contains("Registration Agreement"), "{rendered}");
        assert!(rendered.contains("Resolved Settings:"), "{rendered}");
        assert!(rendered.contains("Jane Smith"), "{rendered}");
        assert!(rendered.contains("PROFILE"), "{rendered}");
    }

    /// Proves `view_columns()` renders a premium domain's `inventory`/`fees`
    /// shaped like what `quote_to_json` actually emits (`fees_to_json`
    /// builds each entry from a `json!` literal with these exact keys) as a
    /// nested table — a mismatch would silently drop them from `--fields
    /// all` output.
    #[test]
    fn quote_result_renders_inventory_and_fees_as_a_nested_table() {
        let quote = json!({
            "domain": "premium-example.com",
            "available": true,
            "inventory": "PREMIUM",
            "fees": [
                {"type": "ONE_TIME_PREMIUM_DOMAIN_PURCHASE", "amount": "3900.00", "currency": "USD"},
            ],
        });
        let envelope = cli_engine::Envelope::success(quote, "domain");
        let rendered = cli_engine::render_human_with_view(&envelope, Some(&view_columns()), "");
        assert!(rendered.contains("Inventory:"), "{rendered}");
        assert!(rendered.contains("PREMIUM"), "{rendered}");
        assert!(rendered.contains("Fees:"), "{rendered}");
        assert!(
            rendered.contains("ONE_TIME_PREMIUM_DOMAIN_PURCHASE"),
            "{rendered}"
        );
        assert!(rendered.contains("3900.00"), "{rendered}");
    }

    /// A non-premium quote (the common case) renders `Inventory:`/`Fees:` as
    /// blank rows, same as every other optional field in this view — but
    /// must never leak a literal `null` (the regression `fees_to_json`'s
    /// `type` key fix targets).
    #[test]
    fn quote_result_never_leaks_a_literal_null_when_fees_are_absent() {
        let quote = json!({
            "domain": "example.com",
            "available": true,
        });
        let envelope = cli_engine::Envelope::success(quote, "domain");
        let rendered = cli_engine::render_human_with_view(&envelope, Some(&view_columns()), "");
        assert!(!rendered.contains("null"), "{rendered}");
    }
}
