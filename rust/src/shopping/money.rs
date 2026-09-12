use serde_json::Value;

use crate::domain::common::format_minor_units;

/// Shopping amounts are ISO-4217 minor units; formatting delegates to the
/// shared Domains currency implementation.
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
    format_minor_units(amount, currency, true, true)
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
    }
}
