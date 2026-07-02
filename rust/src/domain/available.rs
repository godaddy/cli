//! `gddy domain available` — check whether a domain can be registered (v3).

use cli_engine::{
    CommandResult, CommandSpec, NextAction, NextActionParam, RuntimeCommandSpec, Tier,
};
use serde_json::json;

use domains_client::types;

use super::common::{api_error, format_money, make_client};
use crate::output_schema::output_schema;
use crate::scopes::DOMAINS_READ;

// The handler emits a transformed shape (formatted price/currency strings from
// SimpleMoney, a single headline term) rather than the raw `types::Availability`,
// so `--schema`/help metadata is declared to match what's actually returned.
output_schema!(DomainAvailableResult {
    "domain": "string";
    "available": "bool";
    "definitive": "bool";
    // Present only when the API returns a headline price for the term.
    "price": "string", optional;
    "currency": "string", optional;
    "renewalPrice": "string", optional;
    "period": "number", optional;
});

/// The headline price for an availability/quote: the entry for a 1-year term if
/// present, else the first listed term.
fn headline_price(prices: &[types::TermPrice]) -> Option<&types::TermPrice> {
    let one_year = std::num::NonZeroU64::new(1);
    prices
        .iter()
        .find(|p| p.period == one_year)
        .or_else(|| prices.first())
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_with_context(
        CommandSpec::new("available", "Check whether a domain is available")
            .with_long(
                "Check whether a domain can be registered, and at what price. \
                 --check-type fast trades accuracy for speed; full is authoritative \
                 (the `definitive` field tells you which you got).",
            )
            .with_system("domain")
            .with_tier(Tier::Read)
            .with_default_fields("domain,available,definitive,price,currency")
            .with_output_schema::<DomainAvailableResult>()
            .with_scopes(&[DOMAINS_READ])
            .with_arg(
                clap::Arg::new("domain")
                    .value_name("DOMAIN")
                    .required(true)
                    .help("Domain name to check (e.g. example.com)"),
            )
            .with_arg(
                clap::Arg::new("check-type")
                    .long("check-type")
                    .value_name("TYPE")
                    .value_parser(["fast", "full"])
                    .help("Optimize for speed (fast) or accuracy (full)"),
            ),
        |ctx| async move {
            let domain = ctx
                .args
                .get("domain")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_owned();
            // --check-type fast|full → v3 optimizeFor SPEED|ACCURACY.
            let optimize_for = match ctx.args.get("check-type").and_then(|v| v.as_str()) {
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
            let mut result = json!({
                "domain": body.domain.clone().unwrap_or_else(|| domain.clone()),
                "available": body.available.unwrap_or(false),
                "definitive": body.definitive.unwrap_or(false),
            });
            let prices = body.prices.unwrap_or_default();
            if let Some(term) = headline_price(&prices) {
                if let Some(price) = term.price.as_ref().and_then(format_money) {
                    result["price"] = json!(price);
                    result["currency"] = json!(
                        term.price
                            .as_ref()
                            .and_then(|m| m.currency_code.as_ref())
                            .map(|c| c.to_string())
                    );
                }
                if let Some(renewal) = term.renewal_price.as_ref().and_then(format_money) {
                    result["renewalPrice"] = json!(renewal);
                }
                if let Some(period) = term.period {
                    result["period"] = json!(period.get());
                }
            }

            let cmd = CommandResult::new(result);
            if body.available.unwrap_or(false) {
                Ok(cmd.with_next_actions(vec![
                    NextAction::new("domain quote <domain>", "Price a registration")
                        .with_param("domain", NextActionParam::required()),
                ]))
            } else {
                Ok(cmd.with_next_actions(vec![
                    NextAction::new("domain suggest <query>", "Find alternatives")
                        .with_param("query", NextActionParam::required()),
                ]))
            }
        },
    )
}
