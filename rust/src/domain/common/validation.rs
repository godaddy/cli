use cli_engine::{CliCoreError, Result};

/// Validate that `raw` (trimmed) is syntactically a valid domain name. Rejects
/// the shapes from DEVEX-885's bug report — embedded whitespace, null bytes, a
/// "protocol://" prefix, a "/path" suffix — via `url::Host::parse`'s WHATWG
/// forbidden-host-code-point + IDNA checks, then layers RFC 1035/1123 shape
/// rules a real domain always satisfies: not a bare IP literal, >=2
/// dot-separated LDH labels, each 1-63 bytes, total <=253 bytes, and a TLD
/// that isn't all-numeric (ICANN disallows those). Returns the original
/// trimmed input — not `Host::parse`'s ASCII/punycode form — so an
/// already-working Unicode domain's wire format is unchanged.
pub(crate) fn validate_domain_name(raw: &str) -> Result<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(CliCoreError::message("domain name is required"));
    }
    // `url::Host::parse`'s error `Display` is a single generic string
    // ("invalid international domain name") regardless of what's actually
    // wrong (a space, a "://" prefix, a "/path" suffix, ...), so it isn't
    // worth including — it would just read as a confusing non-sequitur.
    let host = url::Host::parse(trimmed)
        .map_err(|_| CliCoreError::message(format!("{trimmed:?} is not a valid domain name")))?;
    let url::Host::Domain(ascii) = host else {
        return Err(CliCoreError::message(format!(
            "{trimmed:?} is an IP address, not a domain name"
        )));
    };
    let labels: Vec<&str> = ascii.split('.').collect();
    let shape_ok = labels.len() >= 2
        && ascii.len() <= 253
        && labels.iter().all(|l| is_ldh_label(l))
        && !labels
            .last()
            .is_some_and(|tld| tld.bytes().all(|b| b.is_ascii_digit()));
    if !shape_ok {
        return Err(CliCoreError::message(format!(
            "{trimmed:?} doesn't look like a valid domain name (expected something like example.com)"
        )));
    }
    Ok(trimmed.to_owned())
}

/// Normalize and validate a TLD label for `--tld` / `--tlds` flags.
///
/// Accepts `com` or `.com` (leading dot stripped), lowercases, and validates
/// each dot-separated label as LDH.
pub(crate) fn validate_tld(raw: &str) -> Result<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(CliCoreError::message("TLD is required"));
    }
    let stripped = trimmed.trim_start_matches('.');
    if stripped.is_empty() {
        return Err(CliCoreError::message(
            "TLD is required (expected something like com, not just a leading dot)",
        ));
    }
    let lower = stripped.to_ascii_lowercase();
    let labels: Vec<&str> = lower.split('.').collect();
    if labels.iter().any(|label| !is_ldh_label(label)) {
        return Err(CliCoreError::message(format!(
            "{raw:?} doesn't look like a valid TLD (expected something like com or co.uk, without a leading dot)"
        )));
    }
    if labels
        .last()
        .is_some_and(|tld| tld.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err(CliCoreError::message(format!("{raw:?} is not a valid TLD")));
    }
    Ok(lower)
}
/// A single RFC 1035/1123 "LDH label": 1-63 bytes, alphanumeric, interior
/// hyphens only (not leading/trailing).
fn is_ldh_label(label: &str) -> bool {
    let bytes = label.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 63
        && bytes[0] != b'-'
        && bytes[bytes.len() - 1] != b'-'
        && bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || *b == b'-')
}
/// Validate a quote token string (non-empty).
pub(crate) fn validate_quote_token(raw: &str) -> Result<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(CliCoreError::message("quote token is required"));
    }
    Ok(trimmed.to_owned())
}
/// Normalize and validate an async operation ID (UUID).
pub(crate) fn validate_operation_id(raw: &str) -> Result<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(CliCoreError::message("operation ID is required"));
    }
    uuid::Uuid::parse_str(trimmed).map_err(|_| {
        CliCoreError::message(format!(
            "{trimmed:?} doesn't look like a valid operation ID (expected a UUID)"
        ))
    })?;
    Ok(trimmed.to_owned())
}
/// Validate every value of a repeatable `--nameserver`-style flag as a
/// domain-shaped hostname, wrapping [`validate_domain_name`]'s generic error
/// with the flag's own name — used by both `quote` (the registration
/// profile's nameservers) and `nameservers set` (the target domain's
/// nameservers) so a bad host isn't reported as if it were some other
/// (already-valid) domain argument.
pub(crate) fn validate_nameserver_hosts(raw: Vec<String>) -> Result<Vec<String>> {
    raw.into_iter()
        .map(|h| {
            validate_domain_name(&h).map_err(|e| CliCoreError::message(format!("--nameserver {e}")))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_domain_name_rejects_devex_885_repro_cases() {
        // Every case from the DEVEX-885 bug report except the trailing-space
        // one (which is accepted-after-trim; see the test below).
        for bad in [
            "not a domain",
            "https://test.com",
            "test.com/page",
            "test\u{0}evil.com",
        ] {
            assert!(
                validate_domain_name(bad).is_err(),
                "{bad:?} should be rejected"
            );
        }
    }

    #[test]
    fn validate_domain_name_trims_whitespace_instead_of_rejecting() {
        assert_eq!(
            validate_domain_name("test.com ").expect("trimmed to valid"),
            "test.com"
        );
        assert_eq!(
            validate_domain_name("  test.com").expect("trimmed to valid"),
            "test.com"
        );
    }

    #[test]
    fn validate_domain_name_rejects_empty_or_whitespace_only() {
        assert!(validate_domain_name("").is_err());
        assert!(validate_domain_name("   ").is_err());
    }

    #[test]
    fn validate_domain_name_rejects_ip_literals() {
        assert!(validate_domain_name("1.2.3.4").is_err());
        assert!(validate_domain_name("::1").is_err());
    }

    #[test]
    fn validate_domain_name_rejects_missing_or_numeric_tld() {
        assert!(validate_domain_name("example").is_err());
        assert!(validate_domain_name("example.123").is_err());
    }

    #[test]
    fn validate_tld_strips_leading_dot_and_lowercases() {
        assert_eq!(validate_tld(".org").expect("valid"), "org");
        assert_eq!(validate_tld("COM").expect("valid"), "com");
        assert_eq!(validate_tld("co.uk").expect("valid"), "co.uk");
    }

    #[test]
    fn validate_tld_rejects_empty_or_dot_only() {
        assert!(validate_tld("").is_err());
        assert!(validate_tld(".").is_err());
        assert!(validate_tld("   ").is_err());
    }
    #[test]
    fn validate_operation_id_accepts_uuid() {
        assert_eq!(
            validate_operation_id("550e8400-e29b-41d4-a716-446655440000").expect("valid"),
            "550e8400-e29b-41d4-a716-446655440000"
        );
    }

    #[test]
    fn validate_operation_id_rejects_garbage() {
        assert!(validate_operation_id("not-a-uuid").is_err());
        assert!(validate_operation_id("").is_err());
    }

    #[test]
    fn validate_domain_name_rejects_leading_or_trailing_hyphen_labels() {
        assert!(validate_domain_name("-example.com").is_err());
        assert!(validate_domain_name("example-.com").is_err());
    }

    #[test]
    fn validate_domain_name_accepts_well_formed_domains_unchanged() {
        for good in ["example.com", "xn--fsq.com", "ns1.example.co.uk"] {
            assert_eq!(validate_domain_name(good).expect("valid domain"), good);
        }
    }

    #[test]
    fn validate_domain_name_accepts_unicode_and_returns_original_not_punycode() {
        // A real Unicode domain must pass (IDNA-processed for the shape check)
        // but the returned string is the user's original input, not
        // `Host::parse`'s ASCII/punycode form — no wire-format change for
        // domains that already work today.
        let input = "café.com";
        assert_eq!(
            validate_domain_name(input).expect("valid unicode domain"),
            input
        );
    }

    #[test]
    fn validate_nameserver_hosts_rejects_bad_shape_with_flag_context() {
        // Regression: `quote`'s and `nameservers set`'s `--nameserver` values
        // must go through the same shape check as a domain arg, but the error
        // must say `--nameserver`, not claim the bad value is "the domain".
        let err = validate_nameserver_hosts(vec!["bad ns".to_string()])
            .expect_err("malformed host should be rejected");
        let msg = err.to_string();
        assert!(msg.starts_with("--nameserver "), "{msg}");
        assert!(msg.contains("bad ns"), "{msg}");
    }

    #[test]
    fn validate_nameserver_hosts_passes_through_valid_hosts_unchanged() {
        let hosts = vec!["ns1.example.com".to_string(), "ns2.example.com".to_string()];
        assert_eq!(
            validate_nameserver_hosts(hosts.clone()).expect("valid hosts"),
            hosts
        );
    }

    #[test]
    fn validate_nameserver_hosts_empty_list_stays_empty() {
        assert_eq!(
            validate_nameserver_hosts(Vec::new()).expect("empty is valid"),
            Vec::<String>::new()
        );
    }
}
