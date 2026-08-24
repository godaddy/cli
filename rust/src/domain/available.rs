//! `gddy domain available` — check whether a domain can be registered (v3).

use cli_engine::{
    Alignment, CommandResult, CommandSpec, NextActionParam, RuntimeCommandSpec, TableColumn, Tier,
};
use serde_json::json;

use domains_client::types;

use super::common::{api_error, format_money, make_client, period_label, validate_domain_name};
use crate::next_action::next_action;
use crate::output_schema::output_schema;
use crate::scopes::DOMAINS_READ;

// The handler emits a transformed shape (formatted price/currency strings from
// SimpleMoney, one entry per registration term) rather than the raw
// `types::Availability`, so `--schema`/help metadata is declared to match what's
// actually returned.
output_schema!(DomainAvailableResult {
    "domain": "string";
    "available": "bool";
    "definitive": "bool";
    // Present only when the API returns priced terms at all.
    "currency": "string", optional;
    "terms": "[]object", optional;
});

fn view_columns() -> Vec<TableColumn> {
    vec![
        TableColumn::new("domain", "Domain"),
        TableColumn::new("available", "Available"),
        TableColumn::new("definitive", "Definitive"),
        TableColumn::new("currency", "Currency"),
        TableColumn::new("terms", "Terms").nested(vec![
            TableColumn::new("periodLabel", "Period"),
            TableColumn::new("price", "Price").align(Alignment::Right),
            TableColumn::new("firstTermPrice", "First-Term Price").align(Alignment::Right),
            TableColumn::new("renewalPrice", "Renewal Price").align(Alignment::Right),
        ]),
    ]
}

/// Render one `TermPrice` entry as `{period, periodLabel, price, renewalPrice,
/// firstTermPrice}`. `None` when the term carries no period (never happens in
/// practice, but keeps every emitted object schema-conformant). Currency isn't
/// repeated per term — it's virtually always the same across a domain's terms,
/// so it's surfaced once at the top level instead of as a redundant column in
/// every row. `TermPrice.recommended` (a hint for web UIs on which term to
/// feature) is intentionally not surfaced — it read as a confusing, unexplained
/// flag in CLI output.
fn term_to_json(term: &types::TermPrice) -> Option<serde_json::Value> {
    let period = term.period?;
    let mut obj = json!({
        "period": period.get(),
        "periodLabel": period_label(period.get()),
    });
    if let Some(price) = term.price.as_ref().and_then(format_money) {
        obj["price"] = json!(price);
    }
    if let Some(renewal) = term.renewal_price.as_ref().and_then(format_money) {
        obj["renewalPrice"] = json!(renewal);
    }
    if let Some(first_term) = term.first_term_price.as_ref().and_then(format_money) {
        obj["firstTermPrice"] = json!(first_term);
    }
    Some(obj)
}

/// The currency code shared by a domain's priced terms (all terms use the
/// same currency in practice, so one top-level field covers every row in
/// `terms`) — the first `currencyCode` found across every term's price,
/// renewal price, and first-term price. Keeps searching past a money value
/// that has no `currencyCode` (itself optional) rather than stopping at the
/// first price-like value regardless of whether it actually carries one.
fn shared_currency(prices: &[types::TermPrice]) -> Option<String> {
    prices
        .iter()
        .flat_map(|t| {
            [
                t.price.as_ref(),
                t.renewal_price.as_ref(),
                t.first_term_price.as_ref(),
            ]
        })
        .flatten()
        .find_map(|m| m.currency_code.as_ref())
        .map(|c| c.to_string())
}

#[derive(Debug, Clone, clap::Args)]
struct AvailableArgs {
    /// Domain name to check (e.g. example.com).
    #[arg(value_name = "DOMAIN")]
    domain: String,

    /// Optimize for speed (fast) or accuracy (full).
    #[arg(long = "check-type", value_name = "TYPE", value_parser = ["fast", "full"])]
    check_type: Option<String>,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<AvailableArgs, _, _, _>(
        CommandSpec::from_args::<AvailableArgs>("available", "Check whether a domain is available")
            .with_long(
                "Check whether a domain can be registered, and at what price. \
                 --check-type fast trades accuracy for speed; full is authoritative \
                 (the `definitive` field tells you which you got). Lists every \
                 registration term the API prices (1 year, 2 years, …), including \
                 any first-term promotion.",
            )
            .with_system("domain")
            .with_tier(Tier::Read)
            .with_default_fields("domain,available,definitive,currency,terms")
            .with_output_schema::<DomainAvailableResult>()
            .with_view(view_columns())
            .with_scopes(&[DOMAINS_READ]),
        |ctx, args: AvailableArgs| async move {
            let domain = validate_domain_name(&args.domain)?;
            // --check-type fast|full → v3 optimizeFor SPEED|ACCURACY.
            let optimize_for = match args.check_type.as_deref() {
                Some("fast") => Some(types::OptimizationTarget::Speed),
                Some("full") => Some(types::OptimizationTarget::Accuracy),
                _ => None,
            };
            let debug = !ctx.middleware.debug.is_empty();
            let client = make_client(&ctx).await?;
            let mut req = client.get_domain_availability().domain(domain.as_str());
            if let Some(opt) = optimize_for {
                req = req.optimize_for(opt);
            }
            let body = match req.send().await {
                Ok(r) => r.into_inner(),
                Err(e) => return Err(api_error("domain availability check", debug, e).await),
            };

            // Emit the required identity fields concretely (never JSON null): the
            // domain falls back to the known input, and missing booleans read as
            // false — matching how availability is treated for the next action.
            let resolved_domain = body.domain.clone().unwrap_or_else(|| domain.clone());
            let mut result = json!({
                "domain": resolved_domain,
                "available": body.available.unwrap_or(false),
                "definitive": body.definitive.unwrap_or(false),
            });
            let prices = body.prices.unwrap_or_default();
            let terms: Vec<serde_json::Value> = prices.iter().filter_map(term_to_json).collect();
            // `terms` is emitted whenever any priced term exists, independent
            // of whether a currency could be derived (`currencyCode` is
            // itself optional per the API, so pricing with no currency is
            // valid and must not suppress the terms themselves). `currency`
            // is the one gated on `terms` being non-empty, so it never
            // appears with no corresponding rows to interpret it against.
            if !terms.is_empty() {
                result["terms"] = json!(terms);
                if let Some(currency) = shared_currency(&prices) {
                    result["currency"] = json!(currency);
                }
            }

            let cmd = CommandResult::new(result);
            if body.available.unwrap_or(false) {
                Ok(cmd.with_next_actions(vec![
                    next_action("domain quote <domain>", "Price a registration")
                        .with_param("domain", NextActionParam::value(resolved_domain)),
                ]))
            } else {
                Ok(cmd.with_next_actions(vec![
                    // `domain suggest` accepts a seed domain, so the domain just
                    // checked as taken is a valid query to copy/paste directly.
                    next_action("domain suggest <query>", "Find alternatives")
                        .with_param("query", NextActionParam::value(resolved_domain)),
                ]))
            }
        },
    )
}

#[cfg(test)]
mod tests {
    use super::{command, shared_currency, view_columns};
    use domains_client::types;
    use serde_json::json;

    #[test]
    fn default_fields_includes_terms() {
        let fields = command().spec.default_fields.expect("default fields set");
        assert!(fields.split(',').any(|f| f == "terms"), "{fields}");
    }

    #[test]
    fn shared_currency_falls_back_to_first_term_price_when_others_are_absent() {
        // A term with only a first-term promotion (no price/renewalPrice) must
        // still yield a currency — otherwise `firstTermPrice` values render
        // with no top-level `currency` to interpret them against.
        let term = types::TermPrice {
            first_term_price: Some(types::SimpleMoney {
                value: Some(250),
                currency_code: Some(types::CurrencyCode("USD".to_string())),
            }),
            ..Default::default()
        };
        assert_eq!(shared_currency(&[term]), Some("USD".to_string()));
    }

    #[test]
    fn shared_currency_keeps_searching_past_a_price_with_no_currency_code() {
        // currencyCode is itself optional per the API. The first term's
        // price having none must not short-circuit the search — a later
        // term (or a later field on the same term) may still carry one.
        let no_currency = types::TermPrice {
            price: Some(types::SimpleMoney {
                value: Some(999),
                currency_code: None,
            }),
            ..Default::default()
        };
        let has_currency = types::TermPrice {
            price: Some(types::SimpleMoney {
                value: Some(1999),
                currency_code: Some(types::CurrencyCode("USD".to_string())),
            }),
            ..Default::default()
        };
        assert_eq!(
            shared_currency(&[no_currency, has_currency]),
            Some("USD".to_string())
        );
    }

    /// Proves `view_columns()` renders `terms` shaped like what the handler
    /// actually emits (confirmed by inspection — `term_to_json` builds each
    /// entry from a `json!` literal with these exact keys) as a nested table,
    /// including the period's human label — a mismatch would silently drop
    /// fields from `--fields all` output.
    #[test]
    fn terms_render_as_a_nested_table_with_period_label() {
        let available = json!({
            "domain": "example.com",
            "available": true,
            "definitive": true,
            "currency": "USD",
            "terms": [
                {
                    "period": 1,
                    "periodLabel": "1 year",
                    "price": "10.49",
                    "renewalPrice": "22.99",
                },
                {
                    "period": 2,
                    "periodLabel": "2 years",
                    "price": "25.48",
                    "renewalPrice": "45.98",
                    "firstTermPrice": "2.50",
                },
            ],
        });
        let envelope = cli_engine::Envelope::success(available, "domain");
        let rendered = cli_engine::render_human_with_view(&envelope, Some(&view_columns()), "");
        assert!(rendered.contains("Terms:"), "{rendered}");
        assert!(rendered.contains("1 year"), "{rendered}");
        assert!(rendered.contains("2 years"), "{rendered}");
        assert!(rendered.contains("2.50"), "{rendered}");
        assert!(!rendered.contains("Recommended"), "{rendered}");
    }
}
