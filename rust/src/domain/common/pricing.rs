use domains_client::types;
use serde_json::json;

/// Appended to the next-step text under every price a domain command shows. The
/// domains API quotes the registration price only: the ICANN fee and any tax
/// are added when the order is placed, so the total charged is higher.
pub(crate) const PRICE_EXCLUDES_TAXES_AND_FEES: &str = "Prices exclude applicable taxes and fees.";

/// The ISO-4217 minor-unit exponent for a currency — how many implied decimal
/// places a [`types::SimpleMoney`] `value` carries (the v3 spec defers money
/// formatting to ISO 4217). Sourced from the `iso_currency` crate's maintained
/// ISO 4217 dataset rather than a hand table, so it stays complete and correct
/// (JPY → 0, USD → 2, KWD → 3, CLF → 4, …).
///
/// Falls back to 2 for an unrecognized code and for the codes ISO marks with no
/// minor unit (precious metals `XAU`/`XAG`, the IMF SDR `XDR`, `XXX`, test codes)
/// — none of which are spendable currencies that could be a domain price.
pub(crate) fn currency_decimals(code: &str) -> u32 {
    iso_currency::Currency::from_code(&code.to_ascii_uppercase())
        .and_then(|c| c.exponent())
        .map_or(2, u32::from)
}

/// Render a [`types::SimpleMoney`] as a decimal string. v3 money `value`s are in
/// ISO-4217 minor units for the currency (e.g. USD `1199` → `"11.99"`, JPY `1500`
/// → `"1500"`, BHD `1234` → `"1.234"`), NOT the micro-units v1 used. Truncates
/// toward zero (registry prices are whole minor units in practice); the sign is
/// explicit and `unsigned_abs` avoids `i64::MIN` overflow. `None` when the amount
/// is absent. Missing currency defaults to 2 decimals.
pub(crate) fn format_money(money: &types::SimpleMoney) -> Option<String> {
    Some(format_minor_units(
        money.value?,
        money
            .currency_code
            .as_ref()
            .map(|code| code.as_str())
            .unwrap_or(""),
        false,
        false,
    ))
}

/// Render a `fees` array (e.g. a premium domain's one-time acquisition
/// surcharge) as `{type, amount, currency}` objects for display. Shared by
/// `available`/`suggest`/`quote`, all of which surface a `types::Fee` list
/// from the domains API. `None` (rather than an empty array) when there are
/// no fees, so the field is omitted from output entirely instead of showing
/// an empty list.
pub(crate) fn fees_to_json(fees: &[types::Fee]) -> Option<serde_json::Value> {
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

/// Formats an ISO-4217 minor-unit amount using the shared currency dataset.
pub(crate) fn format_minor_units(
    value: i64,
    currency: &str,
    include_currency: bool,
    group_whole: bool,
) -> String {
    let decimals = currency_decimals(currency);
    let sign = if value < 0 { "-" } else { "" };
    let absolute = value.unsigned_abs();
    let scale = 10u64.pow(decimals);
    let whole = if group_whole {
        grouped_integer(absolute / scale)
    } else {
        (absolute / scale).to_string()
    };
    let amount = if decimals == 0 {
        format!("{sign}{whole}")
    } else {
        format!(
            "{sign}{whole}.{:0width$}",
            absolute % scale,
            width = decimals as usize
        )
    };
    if include_currency {
        format!("{currency} {amount}")
    } else {
        amount
    }
}

fn grouped_integer(value: u64) -> String {
    let digits = value.to_string();
    let first_group = digits.len() % 3;
    let mut output = String::with_capacity(digits.len() + digits.len() / 3);
    if first_group > 0 {
        output.push_str(&digits[..first_group]);
    }
    for index in (first_group..digits.len()).step_by(3) {
        if !output.is_empty() {
            output.push(',');
        }
        output.push_str(&digits[index..index + 3]);
    }
    output
}

/// The entry for a specific year-term period (1, 2, …), or `None` when that term
/// isn't in the list. Used by `suggest` to flatten multiple terms into scalar
/// per-period fields (e.g. `price1Year`, `price2Year`).
pub(crate) fn term_for_period(
    prices: &[types::TermPrice],
    period: u64,
) -> Option<&types::TermPrice> {
    let period = std::num::NonZeroU64::new(period)?;
    prices.iter().find(|p| p.period == Some(period))
}

/// Sorted, unique registration periods (years) priced in an availability response.
pub(crate) fn periods_from_prices(prices: &[types::TermPrice]) -> Vec<u64> {
    let mut periods: Vec<u64> = prices
        .iter()
        .filter_map(|t| t.period.map(|p| p.get()))
        .collect();
    periods.sort_unstable();
    periods.dedup();
    periods
}

/// Indicative total registration price keyed by period years.
pub(crate) fn period_price_map(
    prices: &[types::TermPrice],
) -> std::collections::BTreeMap<u64, String> {
    let mut map = std::collections::BTreeMap::new();
    for term in prices {
        let Some(period) = term.period.map(|p| p.get()) else {
            continue;
        };
        if let Some(price) = term.price.as_ref().and_then(format_money) {
            map.insert(period, price);
        }
    }
    map
}

/// Whether an API error body indicates the requested registration period exceeds
/// the TLD limit. Availability pricing is indicative; quote is authoritative.
pub(crate) fn is_period_limit_error(body: &str) -> bool {
    let lower = body.to_ascii_lowercase();
    lower.contains("not currently supported") || lower.contains("maximum is")
}

/// Parse the maximum registration period from a period-limit error body.
pub(crate) fn parse_max_registration_period(body: &str) -> Option<u64> {
    let lower = body.to_ascii_lowercase();
    let needle = "maximum is ";
    let rest = lower.split(needle).nth(1)?;
    rest.split_whitespace().next()?.parse().ok()
}

/// Drop unsupported periods and clamp the selected period to the TLD maximum.
pub(crate) fn clamp_registration_periods(
    available_periods: &mut Vec<u64>,
    selected_period: &mut u64,
    max_years: u64,
) {
    available_periods.retain(|p| *p <= max_years);
    if available_periods.is_empty() {
        available_periods.push(1);
    }
    if *selected_period > max_years {
        *selected_period = available_periods.iter().copied().max().unwrap_or(1);
    }
}

/// A registration length with its unit spelled out ("1 year", "2 years") — a
/// bare number reads ambiguously in a table, so `quote`/`available` show this
/// alongside the numeric `period` field (which stays a plain number for
/// scripting against `--output json`).
pub(crate) fn period_label(period: u64) -> String {
    if period == 1 {
        "1 year".to_owned()
    } else {
        format!("{period} years")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn money(value: Option<i64>, currency: &str) -> types::SimpleMoney {
        types::SimpleMoney {
            value,
            currency_code: (!currency.is_empty())
                .then(|| types::CurrencyCode(currency.to_string())),
        }
    }

    #[test]
    fn periods_from_prices_returns_sorted_unique_periods() {
        let prices = vec![
            types::TermPrice {
                period: std::num::NonZeroU64::new(3),
                ..Default::default()
            },
            types::TermPrice {
                period: std::num::NonZeroU64::new(1),
                ..Default::default()
            },
            types::TermPrice {
                period: std::num::NonZeroU64::new(2),
                ..Default::default()
            },
            types::TermPrice {
                period: std::num::NonZeroU64::new(2),
                ..Default::default()
            },
        ];
        assert_eq!(periods_from_prices(&prices), vec![1, 2, 3]);
    }

    #[test]
    fn period_price_map_formats_indicative_totals() {
        let prices = vec![types::TermPrice {
            period: std::num::NonZeroU64::new(2),
            price: Some(types::SimpleMoney {
                value: Some(6098),
                currency_code: Some(types::CurrencyCode("USD".to_string())),
            }),
            ..Default::default()
        }];
        let map = period_price_map(&prices);
        assert_eq!(map.get(&2).map(String::as_str), Some("60.98"));
    }

    #[test]
    fn parse_max_registration_period_reads_api_error_text() {
        let body = r#"{"details":[{"description":"period 5 is not currently supported; maximum is 3 years"}]}"#;
        assert!(is_period_limit_error(body));
        assert_eq!(parse_max_registration_period(body), Some(3));
    }

    #[test]
    fn clamp_registration_periods_drops_and_clamps_selection() {
        let mut periods = vec![1_u64, 2, 3, 5];
        let mut selected = 5_u64;
        clamp_registration_periods(&mut periods, &mut selected, 3);
        assert_eq!(periods, vec![1, 2, 3]);
        assert_eq!(selected, 3);
    }

    #[test]
    fn period_label_pluralizes_correctly() {
        assert_eq!(period_label(1), "1 year");
        assert_eq!(period_label(2), "2 years");
        assert_eq!(period_label(10), "10 years");
    }

    #[test]
    fn fees_to_json_renders_type_amount_and_currency() {
        let fee = types::Fee {
            fee: Some(money(Some(390000), "USD")),
            type_: Some(types::FeeType(
                "ONE_TIME_PREMIUM_DOMAIN_PURCHASE".to_string(),
            )),
        };
        let rendered = fees_to_json(&[fee]).expect("non-empty fees render Some");
        assert_eq!(
            rendered,
            json!([{
                "type": "ONE_TIME_PREMIUM_DOMAIN_PURCHASE",
                "amount": "3900.00",
                "currency": "USD",
            }])
        );
    }

    #[test]
    fn fees_to_json_is_none_for_an_empty_list() {
        // `available`/`suggest`/`quote` all gate on this to omit `fees`
        // entirely rather than emit an empty array.
        assert_eq!(fees_to_json(&[]), None);
    }

    #[test]
    fn format_money_uses_iso4217_minor_units_per_currency() {
        // v3 `value` is in the currency's ISO-4217 minor units — USD `1199` is
        // $11.99, NOT micro-units (the regression that rendered real prices as 0.00).
        assert_eq!(
            format_money(&money(Some(1199), "USD")).as_deref(),
            Some("11.99")
        );
        assert_eq!(
            format_money(&money(Some(100), "USD")).as_deref(),
            Some("1.00")
        );
        assert_eq!(
            format_money(&money(Some(2050), "USD")).as_deref(),
            Some("20.50")
        );
        assert_eq!(
            format_money(&money(Some(-500), "USD")).as_deref(),
            Some("-5.00")
        );
        // Zero-decimal (JPY) and three-decimal (BHD) currencies.
        assert_eq!(
            format_money(&money(Some(1500), "JPY")).as_deref(),
            Some("1500")
        );
        assert_eq!(
            format_money(&money(Some(1234), "BHD")).as_deref(),
            Some("1.234")
        );
        // Missing amount → None; missing currency defaults to 2 decimals.
        assert_eq!(format_money(&money(None, "USD")), None);
        assert_eq!(
            format_money(&money(Some(1199), "")).as_deref(),
            Some("11.99")
        );
    }

    #[test]
    fn currency_decimals_covers_iso_exceptions() {
        assert_eq!(currency_decimals("USD"), 2);
        assert_eq!(currency_decimals("EUR"), 2);
        assert_eq!(currency_decimals("JPY"), 0);
        assert_eq!(currency_decimals("UYI"), 0);
        assert_eq!(currency_decimals("KWD"), 3);
        assert_eq!(currency_decimals("CLF"), 4);
        assert_eq!(currency_decimals("UYW"), 4);
        assert_eq!(currency_decimals("XAU"), 2); // no ISO minor unit → default 2 (never a price)
        assert_eq!(currency_decimals("ZZZ"), 2); // unknown → default 2
    }
}
