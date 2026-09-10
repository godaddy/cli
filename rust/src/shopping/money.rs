use serde_json::Value;

/// Shopping amounts are ISO-4217 minor units. Their decimal scale derives from the returned
/// currency code, not a fixed cents assumption.
pub(crate) fn format_value(value: Option<&Value>) -> Option<String> {
    let amount = value?.get("amount")?.as_i64()?;
    let currency = value?.get("currency")?.as_str()?;
    Some(format_amount(amount, currency))
}

pub(crate) fn format_total(totals: Option<&Value>, currency: Option<&str>) -> Option<String> {
    let currency = currency?;
    let amount = totals?
        .as_array()?
        .iter()
        .find(|total| total.get("type").and_then(Value::as_str) == Some("total"))?
        .get("amount")?
        .as_i64()?;
    Some(format_amount(amount, currency))
}

pub(crate) fn format_amount(amount: i64, currency: &str) -> String {
    let sign = if amount < 0 { "-" } else { "" };
    let absolute = amount.unsigned_abs();
    let decimals = currency_decimals(currency);
    let scale = 10u64.pow(decimals);
    let whole = grouped_integer(absolute / scale);
    if decimals == 0 {
        format!("{currency} {sign}{whole}")
    } else {
        format!(
            "{currency} {sign}{whole}.{:0width$}",
            absolute % scale,
            width = decimals as usize
        )
    }
}

fn currency_decimals(currency: &str) -> u32 {
    iso_currency::Currency::from_code(&currency.to_ascii_uppercase())
        .and_then(|currency| currency.exponent())
        .map_or(2, u32::from)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_iso4217_minor_units() {
        assert_eq!(format_amount(7188, "USD"), "USD 71.88");
        assert_eq!(format_amount(5988, "GBP"), "GBP 59.88");
        assert_eq!(format_amount(11_988, "JPY"), "JPY 11,988");
        assert_eq!(format_amount(1_234, "KWD"), "KWD 1.234");
        assert_eq!(format_amount(12_345_678, "CLF"), "CLF 1,234.5678");
    }

    #[test]
    fn formats_total_only_when_currency_and_total_are_present() {
        assert_eq!(
            format_total(
                Some(&serde_json::json!([
                    {"type": "subtotal", "amount": 7188},
                    {"type": "total", "amount": 7988}
                ])),
                Some("USD")
            ),
            Some("USD 79.88".to_owned())
        );
        assert_eq!(
            format_total(
                Some(&serde_json::json!([{"type": "total", "amount": 7988}])),
                None
            ),
            None
        );
        assert_eq!(
            format_total(
                Some(&serde_json::json!([{"type": "subtotal", "amount": 7188}])),
                Some("USD")
            ),
            None
        );
    }

    #[test]
    fn formats_negative_and_unknown_currency_amounts() {
        assert_eq!(format_amount(-500, "USD"), "USD -5.00");
        assert_eq!(format_amount(1_234, "ZZZ"), "ZZZ 12.34");
    }
}
