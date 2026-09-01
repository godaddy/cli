//! Shared helpers for the `domain` command group: the authenticated Domains API
//! client, money formatting, argument helpers, and API-error rendering. Each
//! `gddy domain` subcommand lives in its own sibling module and draws from here.


mod client;
mod errors;
mod interactive;
mod operation;
mod pricing;
mod validation;

pub(crate) use client::{make_client, make_client_with_cred};
pub(crate) use errors::{api_error, format_api_error};
pub(super) use interactive::{
    fetch_with_domain_retry, fetch_with_operation_retry, prompt_validated_domain_name,
    prompt_validated_quote_token, prompt_validated_tld, recoverable_tld_api_error,
    resolve_domain_name, resolve_nameserver_hosts, resolve_operation_id, resolve_optional_tlds,
    resolve_quote_token, resolve_tlds,
};
pub(super) use operation::{format_operation_error, is_terminal_status};
pub(super) use pricing::{
    clamp_registration_periods, format_money, is_period_limit_error, parse_max_registration_period,
    period_label, period_price_map, periods_from_prices, term_for_period,
};
pub(super) use validation::{validate_domain_name, validate_nameserver_hosts};

/// Collapse a repeatable CLI flag's values into the single query-string value
/// some domains-client endpoints require (e.g. `tlds`, whose OpenAPI param is
/// `style: form, explode: false` — one comma-joined value). progenitor's
/// generated setters always seq-serialize a `Vec` argument as repeated
/// `key=value` pairs regardless of the spec's `explode` setting, so passing
/// multiple `--tlds` occurrences straight through sends `tlds=com&tlds=net`
/// and the API rejects it (`400 MISMATCH_FORMAT`, DEVEX-882). Joining into a
/// single element before calling the setter produces the one pair the API
/// expects. `[]` stays `[]` so callers can still gate on "no filter given".
pub(crate) fn comma_joined(values: Vec<String>) -> Vec<String> {
    if values.len() <= 1 {
        values
    } else {
        vec![values.join(",")]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comma_joined_collapses_multiple_values_into_one_element() {
        // Regression for DEVEX-882: repeated `--tlds`/`--tld` values must become
        // one comma-joined query element, not stay as separate elements (which
        // progenitor would send as repeated `tlds=` pairs).
        assert_eq!(
            comma_joined(vec!["com".to_string(), "net".to_string(), "io".to_string()]),
            vec!["com,net,io".to_string()]
        );
    }

    #[test]
    fn comma_joined_single_value_is_unchanged() {
        assert_eq!(
            comma_joined(vec!["com".to_string()]),
            vec!["com".to_string()]
        );
    }

    #[test]
    fn comma_joined_empty_stays_empty() {
        assert_eq!(comma_joined(Vec::<String>::new()), Vec::<String>::new());
    }

}
