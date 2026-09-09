use serde_json::Value;

/// Shopping currently returns integer price amounts in hundredths for every observed currency.
/// The service does not yet apply ISO 4217 currency-specific minor-unit exponents.
pub(crate) fn format_value(value: Option<&Value>) -> Option<String> {
    let amount = value?.get("amount")?.as_i64()?;
    let currency = value?.get("currency")?.as_str()?;
    Some(format_amount(amount, currency))
}

pub(crate) fn format_amount(amount: i64, currency: &str) -> String {
    let sign = if amount < 0 { "-" } else { "" };
    let absolute = amount.unsigned_abs();
    let whole = absolute / 100;
    let fractional = absolute % 100;
    let whole = grouped_integer(whole);
    if fractional == 0 && uses_zero_decimal_display(currency) {
        format!("{currency} {sign}{whole}")
    } else {
        format!("{currency} {sign}{whole}.{fractional:02}")
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

fn uses_zero_decimal_display(currency: &str) -> bool {
    matches!(currency, "JPY")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_observed_usd_and_jpy_amounts() {
        assert_eq!(format_amount(7188, "USD"), "USD 71.88");
        assert_eq!(format_amount(1_198_800, "JPY"), "JPY 11,988");
    }

    #[test]
    fn retains_fractional_amounts_for_zero_decimal_display_currencies() {
        assert_eq!(format_amount(1_198_801, "JPY"), "JPY 11,988.01");
    }
}
