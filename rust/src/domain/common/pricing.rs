use domains_client::types;

/// The ISO-4217 minor-unit exponent for a currency — how many implied decimal
/// places a [`types::SimpleMoney`] `value` carries (the v3 spec defers money
/// formatting to ISO 4217). Sourced from the `iso_currency` crate's maintained
/// ISO 4217 dataset rather than a hand table, so it stays complete and correct
/// (JPY → 0, USD → 2, KWD → 3, CLF → 4, …).
///
/// Falls back to 2 for an unrecognized code and for the codes ISO marks with no
/// minor unit (precious metals `XAU`/`XAG`, the IMF SDR `XDR`, `XXX`, test codes)
/// — none of which are spendable currencies that could be a domain price.
fn currency_decimals(code: &str) -> u32 {
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
    let value = money.value?;
    let code = money
        .currency_code
        .as_ref()
        .map(|c| c.as_str())
        .unwrap_or("");
    let decimals = currency_decimals(code);
    let sign = if value < 0 { "-" } else { "" };
    let abs = value.unsigned_abs();
    if decimals == 0 {
        return Some(format!("{sign}{abs}"));
    }
    let scale = 10u64.pow(decimals);
    Some(format!(
        "{sign}{}.{:0width$}",
        abs / scale,
        abs % scale,
        width = decimals as usize
    ))
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
    use domains_client::types;

    #[test]
    fn periods_from_prices_returns_sorted_unique_periods() {
        use domains_client::types;

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
        use domains_client::types;

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
}
