//! Time values: plain seconds or BIND-style units.

/// The largest TTL RFC 2181 §8 allows.
const MAX_TTL: u64 = (1 << 31) - 1;

/// Read a TTL: seconds (`3600`) or units (`1h`, `30m`, `1d12h`, `2w`). A unit is
/// `s`, `m`, `h`, `d` or `w`, in either case.
pub(crate) fn parse_ttl(text: &str) -> Result<u32, String> {
    let bad = || format!("`{text}` is not a time; use seconds or units such as `1h30m`");
    let total = if text.bytes().all(|b| b.is_ascii_digit()) {
        text.parse::<u64>().map_err(|_| bad())?
    } else {
        let mut total: u64 = 0;
        let mut digits = String::new();
        for ch in text.chars() {
            if ch.is_ascii_digit() {
                digits.push(ch);
                continue;
            }
            let unit: u64 = match ch.to_ascii_lowercase() {
                's' => 1,
                'm' => 60,
                'h' => 3600,
                'd' => 86_400,
                'w' => 604_800,
                _ => return Err(bad()),
            };
            let count = digits.parse::<u64>().map_err(|_| bad())?;
            digits.clear();
            total = count
                .checked_mul(unit)
                .and_then(|part| total.checked_add(part))
                .ok_or_else(bad)?;
        }
        if !digits.is_empty() {
            return Err(bad());
        }
        total
    };
    if total > MAX_TTL {
        return Err(format!(
            "`{text}` is too long; a time is at most {MAX_TTL} seconds"
        ));
    }
    u32::try_from(total).map_err(|_| bad())
}

#[cfg(test)]
mod tests {
    use super::parse_ttl;

    #[test]
    fn reads_seconds_and_units() {
        assert_eq!(parse_ttl("0"), Ok(0));
        assert_eq!(parse_ttl("3600"), Ok(3600));
        assert_eq!(parse_ttl("1h"), Ok(3600));
        assert_eq!(parse_ttl("1H30m"), Ok(5400));
        assert_eq!(parse_ttl("1d12h"), Ok(129_600));
        assert_eq!(parse_ttl("2w"), Ok(1_209_600));
        assert_eq!(parse_ttl("90s"), Ok(90));
    }

    #[test]
    fn rejects_malformed_and_oversized_times() {
        for text in ["", "h", "1h30", "1x", "-5", "1.5h", "1 h"] {
            assert!(parse_ttl(text).is_err(), "{text:?} should be rejected");
        }
        assert_eq!(parse_ttl("2147483647"), Ok(2_147_483_647));
        assert!(parse_ttl("2147483648").is_err());
        assert!(parse_ttl("99999999999999999999").is_err());
        assert!(parse_ttl("99999999999w").is_err());
    }
}
