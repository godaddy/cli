//! `gddy domain available` — check whether a domain can be registered (v3).

use cli_engine::{
    CommandResult, CommandSpec, NextActionParam, RuntimeCommandSpec, TableColumn, Tier,
};
use serde_json::json;

use domains_client::types;

use super::common::{
    api_error, format_money, headline_price, make_client, period_label, validate_domain_name,
};
use crate::next_action::next_action;
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
    "periodLabel": "string", optional;
});

/// `period` alone would render in the default table as a bare number, so swap
/// it for the unit-bearing `periodLabel` there; `period` stays available (via
/// `--fields all`/explicit selection) for scripting against `--output json`.
fn view_columns() -> Vec<TableColumn> {
    vec![
        TableColumn::new("domain", "Domain"),
        TableColumn::new("available", "Available"),
        TableColumn::new("definitive", "Definitive"),
        TableColumn::new("price", "Price"),
        TableColumn::new("renewalPrice", "Renewal Price"),
        TableColumn::new("currency", "Currency"),
        TableColumn::new("periodLabel", "Period"),
    ]
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

/// The headline price for an availability/quote: the entry for a 1-year term if
/// present, else the first listed term.
pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<AvailableArgs, _, _, _>(
        CommandSpec::from_args::<AvailableArgs>("available", "Check whether a domain is available")
            .with_long(
                "Check whether a domain can be registered, and at what price. \
                 --check-type fast trades accuracy for speed; full is authoritative \
                 (the `definitive` field tells you which you got).",
            )
            .with_system("domain")
            .with_tier(Tier::Read)
            .with_default_fields(
                "domain,available,definitive,price,renewalPrice,currency,period,periodLabel",
            )
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
            if let Some(term) = headline_price(&prices) {
                if let Some(price) = term.price.as_ref().and_then(format_money) {
                    result["price"] = json!(price);
                    // Only emit `currency` when a code is present — never a JSON
                    // null (schema declares it optional string; null leaks into
                    // table output).
                    if let Some(code) = term.price.as_ref().and_then(|m| m.currency_code.as_ref()) {
                        result["currency"] = json!(code.to_string());
                    }
                }
                if let Some(renewal) = term.renewal_price.as_ref().and_then(format_money) {
                    result["renewalPrice"] = json!(renewal);
                }
                if let Some(period) = term.period {
                    result["period"] = json!(period.get());
                    result["periodLabel"] = json!(period_label(period.get()));
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
        let available = json!({
            "domain": "example.com",
            "available": true,
            "definitive": true,
            "period": 1,
            "periodLabel": "1 year",
        });
        let envelope = cli_engine::Envelope::success(available, "domain");
        let rendered = cli_engine::render_human_with_view(&envelope, Some(&view_columns()), "");
        assert!(rendered.contains("1 year"), "{rendered}");
    }
}
