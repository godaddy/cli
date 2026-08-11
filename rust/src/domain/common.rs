//! Shared helpers for the `domain` command group: the authenticated Domains API
//! client, money formatting, argument helpers, and API-error rendering. Each
//! `gddy domain` subcommand lives in its own sibling module and draws from here.

use cli_engine::{CliCoreError, CommandContext, Credential, Result};

use crate::environments;

use domains_client::types;

const USER_AGENT: &str = concat!("godaddy-cli/", env!("CARGO_PKG_VERSION"));

/// Bridges domains-client's request/response observations into cli-engine's
/// `--debug transport` logger. domains-client defines the `TransportObserver`
/// extension point itself and has no compile-time dependency on cli-engine —
/// this crate *pushes* the logging behavior in, rather than domains-client
/// *pulling* it from the engine.
struct CliEngineTransportObserver;

impl domains_client::TransportObserver for CliEngineTransportObserver {
    fn on_request(&self, request: &reqwest::Request) {
        cli_engine::transport::debug_log_reqwest_request(request);
    }

    fn on_response(&self, status: reqwest::StatusCode, headers: &reqwest::header::HeaderMap) {
        cli_engine::transport::debug_log_reqwest_response(status, headers, &[]);
    }
}

static TRANSPORT_OBSERVER_INIT: std::sync::Once = std::sync::Once::new();

fn ensure_transport_observer_registered() {
    TRANSPORT_OBSERVER_INIT.call_once(|| {
        domains_client::set_transport_observer(Some(std::sync::Arc::new(
            CliEngineTransportObserver,
        )));
    });
}

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
pub(super) fn format_money(money: &types::SimpleMoney) -> Option<String> {
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

/// The headline price among per-term prices: the 1-year term if present, else the
/// first listed term. Used by `available`, which surfaces a single indicative
/// price from a `TermPrice` array.
pub(super) fn headline_price(prices: &[types::TermPrice]) -> Option<&types::TermPrice> {
    let one_year = std::num::NonZeroU64::new(1);
    prices
        .iter()
        .find(|p| p.period == one_year)
        .or_else(|| prices.first())
}

/// The entry for a specific year-term period (1, 2, …), or `None` when that term
/// isn't in the list. Used by `suggest` to flatten multiple terms into scalar
/// per-period fields (e.g. `price1Year`, `price2Year`).
pub(super) fn term_for_period(
    prices: &[types::TermPrice],
    period: u64,
) -> Option<&types::TermPrice> {
    let period = std::num::NonZeroU64::new(period)?;
    prices.iter().find(|p| p.period == Some(period))
}

/// A registration length with its unit spelled out ("1 year", "2 years") — a
/// bare number reads ambiguously in a table, so `quote`/`available` show this
/// alongside the numeric `period` field (which stays a plain number for
/// scripting against `--output json`).
pub(super) fn period_label(period: u64) -> String {
    if period == 1 {
        "1 year".to_owned()
    } else {
        format!("{period} years")
    }
}

/// Validate that `raw` (trimmed) is syntactically a valid domain name. Rejects
/// the shapes from DEVEX-885's bug report — embedded whitespace, null bytes, a
/// "protocol://" prefix, a "/path" suffix — via `url::Host::parse`'s WHATWG
/// forbidden-host-code-point + IDNA checks, then layers RFC 1035/1123 shape
/// rules a real domain always satisfies: not a bare IP literal, >=2
/// dot-separated LDH labels, each 1-63 bytes, total <=253 bytes, and a TLD
/// that isn't all-numeric (ICANN disallows those). Returns the original
/// trimmed input — not `Host::parse`'s ASCII/punycode form — so an
/// already-working Unicode domain's wire format is unchanged.
pub(super) fn validate_domain_name(raw: &str) -> Result<String> {
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

/// Validate every value of a repeatable `--nameserver`-style flag as a
/// domain-shaped hostname, wrapping [`validate_domain_name`]'s generic error
/// with the flag's own name — used by both `quote` (the registration
/// profile's nameservers) and `nameservers set` (the target domain's
/// nameservers) so a bad host isn't reported as if it were some other
/// (already-valid) domain argument.
pub(super) fn validate_nameserver_hosts(raw: Vec<String>) -> Result<Vec<String>> {
    raw.into_iter()
        .map(|h| {
            validate_domain_name(&h).map_err(|e| CliCoreError::message(format!("--nameserver {e}")))
        })
        .collect()
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

/// Whether an async domain-operation status is terminal (no further polling).
/// Only `COMPLETED`/`FAILED` are terminal; every other status (e.g. `SUBMITTED`,
/// `PENDING`, `CONFIRMED`, `EXECUTING`) is treated as still in progress.
pub(super) fn is_terminal_status(status: &str) -> bool {
    matches!(status, "COMPLETED" | "FAILED")
}

/// Renders a `FAILED` operation's `error` payload (name/message) for the
/// user-facing error, e.g. `" (DOMAIN_UNAVAILABLE: the domain is already
/// registered)"`. Empty when the operation carried no error detail (e.g. it
/// failed before an operation with a populated `error` was ever polled).
pub(super) fn format_operation_error(error: Option<&types::Error>) -> String {
    let Some(error) = error else {
        return String::new();
    };
    match (error.name.as_deref(), error.message.as_deref()) {
        (Some(name), Some(message)) => format!(" ({name}: {message})"),
        (Some(name), None) => format!(" ({name})"),
        (None, Some(message)) => format!(" ({message})"),
        (None, None) => String::new(),
    }
}

/// Build a Domains API client for the active environment, authenticating with
/// the resolved OAuth bearer token.
pub(crate) async fn make_client(ctx: &CommandContext) -> Result<domains_client::Client> {
    let cred = ctx.credential().await?;
    make_client_with_cred(&ctx.middleware.env, &cred)
}

/// Build the Domains API client from an already-resolved credential, so callers
/// that need the credential themselves (e.g. `purchase`, for the consent
/// principal) resolve it once and reuse the same token for the requests.
pub(crate) fn make_client_with_cred(
    env: &str,
    cred: &Credential,
) -> Result<domains_client::Client> {
    ensure_transport_observer_registered();
    let config = environments::resolve(env)?;
    let authorization = format!("Bearer {}", cred.token);
    let request_id = uuid::Uuid::new_v4().to_string();
    domains_client::client_with_auth(
        &config.domains_api_url,
        &authorization,
        USER_AGENT,
        &request_id,
    )
    .map_err(|e| CliCoreError::message(format!("failed to build domains client: {e}")))
}

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

/// Turn a domains-client error into a `CliCoreError`, reading the response body
/// for unexpected (non-2xx) responses so the API's actual message isn't lost
/// (progenitor's `Display` prints only the status). Async because reading the
/// body is async.
pub(crate) async fn api_error(
    action: &str,
    debug: bool,
    err: domains_client::Error<()>,
) -> CliCoreError {
    match err {
        domains_client::Error::UnexpectedResponse(resp) => {
            let status = resp.status();
            let request_id = resp
                .headers()
                .get("x-request-id")
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned);
            let body = resp.text().await.unwrap_or_default();
            CliCoreError::message(format_api_error(
                action,
                status.as_u16(),
                &status.to_string(),
                &body,
                request_id.as_deref(),
                debug,
            ))
        }
        other => CliCoreError::message(format!("{action} failed: {other}")),
    }
}

/// Build the user-facing message for an unexpected API response. Pure so the
/// HTTP 402 → payment-method guidance and the `--debug` request-id line are
/// unit-testable.
pub(crate) fn format_api_error(
    action: &str,
    status: u16,
    status_display: &str,
    body: &str,
    request_id: Option<&str>,
    debug: bool,
) -> String {
    let body = body.trim();
    let friendly = friendly_field_errors(body);
    let mut msg = match &friendly {
        Some(detail) => format!("{action} failed (HTTP {status_display}):\n{detail}"),
        None if body.is_empty() => format!("{action} failed (HTTP {status_display})"),
        None => format!("{action} failed (HTTP {status_display}): {body}"),
    };
    if status == 402 {
        msg.push_str(
            "\n\nThis usually means your account has no usable payment method. Add one with \
             `gddy payment-methods add` (a credit card or Good-as-Gold balance is required for domain \
             purchases), then try again.",
        );
    }
    if debug {
        if friendly.is_some() && !body.is_empty() {
            msg.push_str(&format!("\n\nResponse body: {body}"));
        }
        if let Some(id) = request_id.map(str::trim).filter(|s| !s.is_empty()) {
            msg.push_str(&format!("\n\nRequest ID: {id}"));
        }
    }
    msg
}

/// A Domains API validation error body. Tolerates the v1 field-level shape
/// (`{"fields":[...]}` or `{"error":{"fields":[...]}}`) and the v3 top-level
/// shape (`{"name","message","correlationId","details":[{"issue","description"}]}`).
#[derive(serde::Deserialize)]
struct ApiErrorBody {
    #[serde(default)]
    fields: Vec<ApiFieldError>,
    #[serde(default)]
    error: Option<ApiErrorEnvelope>,
    #[serde(default)]
    details: Vec<ApiDetailError>,
}

#[derive(serde::Deserialize)]
struct ApiErrorEnvelope {
    #[serde(default)]
    fields: Vec<ApiFieldError>,
}

#[derive(serde::Deserialize)]
struct ApiFieldError {
    #[serde(default)]
    code: String,
    #[serde(default)]
    path: String,
}

#[derive(serde::Deserialize)]
struct ApiDetailError {
    #[serde(default)]
    description: String,
    #[serde(default)]
    issue: String,
}

impl ApiDetailError {
    /// The human-readable `description` when present; the v3 spec documents it
    /// as unstable ("MAY change... MUST NOT depend on this value") and often
    /// absent, so fall back to the stable `issue` code rather than dropping the
    /// detail entirely.
    fn render(&self) -> Option<&str> {
        if !self.description.is_empty() {
            Some(self.description.as_str())
        } else if !self.issue.is_empty() {
            Some(self.issue.as_str())
        } else {
            None
        }
    }
}

/// Render a structured validation error as a plain-English bullet list, or `None`
/// when `body` isn't a field-level validation error.
fn friendly_field_errors(body: &str) -> Option<String> {
    let parsed = serde_json::from_str::<ApiErrorBody>(body).ok()?;
    let fields = if !parsed.fields.is_empty() {
        parsed.fields
    } else {
        parsed.error.map(|e| e.fields).unwrap_or_default()
    };
    if !fields.is_empty() {
        let lines: Vec<String> = fields
            .iter()
            .map(|f| format!("  • {}", describe_field_error(f)))
            .collect();
        return Some(format!("some fields are invalid:\n{}", lines.join("\n")));
    }
    if !parsed.details.is_empty() {
        let lines: Vec<String> = parsed
            .details
            .iter()
            .filter_map(ApiDetailError::render)
            .map(|text| format!("  • {text}"))
            .collect();
        if !lines.is_empty() {
            return Some(format!("some fields are invalid:\n{}", lines.join("\n")));
        }
    }
    None
}

fn describe_field_error(f: &ApiFieldError) -> String {
    let name = friendly_field_name(&f.path);
    let problem = match f.code.as_str() {
        "LENGTH_UNDER" | "MIN_LENGTH" | "TOO_SHORT" => "is too short",
        "LENGTH_OVER" | "MAX_LENGTH" | "TOO_LONG" => "is too long",
        "PATTERN" | "INVALID_FORMAT" | "INVALID_PATTERN" => "is not in the required format",
        "REQUIRED" | "MISSING" | "MISSING_REQUIRED_FIELD" => "is required",
        _ => "is invalid",
    };
    match field_hint(&f.path, &f.code) {
        Some(hint) => format!("{name} {problem} ({hint})"),
        None => format!("{name} {problem}"),
    }
}

fn field_hint(path: &str, code: &str) -> Option<&'static str> {
    let leaf = path.rsplit('.').next().unwrap_or(path);
    match (leaf, code) {
        ("phone", _) | ("nationalNumber", _) => Some("expected a format like +1.4805551212"),
        ("postalCode", "PATTERN" | "INVALID_FORMAT" | "INVALID_PATTERN") => {
            Some("some countries require a specific format, e.g. UK SW1A 2AA")
        }
        ("line2", "LENGTH_UNDER" | "MIN_LENGTH" | "TOO_SHORT") => Some("leave it blank to omit it"),
        _ => None,
    }
}

/// Turn an API field path into a human phrase. Falls back to a space-joined,
/// camelCase-split rendering for unrecognized paths.
fn friendly_field_name(path: &str) -> String {
    let trimmed = path
        .strip_prefix("body.")
        .or_else(|| path.strip_prefix("consent."))
        .unwrap_or(path);
    let segments: Vec<&str> = trimmed.split('.').filter(|s| !s.is_empty()).collect();
    if let ["contacts", role, rest @ ..] = segments.as_slice() {
        let mut parts = vec![humanize_segment(role)];
        parts.extend(rest.iter().map(|s| humanize_segment(s)));
        return parts.join(" ");
    }
    if segments.is_empty() {
        return path.to_string();
    }
    segments
        .iter()
        .map(|s| humanize_segment(s))
        .collect::<Vec<_>>()
        .join(" ")
}

fn humanize_segment(seg: &str) -> String {
    match seg {
        "line1" => "line 1".to_string(),
        "line2" => "line 2".to_string(),
        "postalCode" => "postal code".to_string(),
        "countryCode" => "country".to_string(),
        "firstName" => "first name".to_string(),
        "lastName" => "last name".to_string(),
        "nationalNumber" => "phone".to_string(),
        "agreedAt" => "agreed-at timestamp".to_string(),
        other => split_camel_case(other),
    }
}

fn split_camel_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    for (i, ch) in s.char_indices() {
        if ch.is_ascii_uppercase() {
            if i != 0 {
                out.push(' ');
            }
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
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

    fn money(value: Option<i64>, currency: &str) -> types::SimpleMoney {
        types::SimpleMoney {
            value,
            currency_code: (!currency.is_empty())
                .then(|| types::CurrencyCode(currency.to_string())),
        }
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

    #[test]
    fn payment_required_error_points_to_payment_methods_add() {
        let msg = format_api_error(
            "domain purchase",
            402,
            "402 Payment Required",
            r#"{"code":"INVALID_PAYMENT_INFO","message":"Unable to authorize credit"}"#,
            None,
            false,
        );
        assert!(msg.contains("402 Payment Required"), "{msg}");
        assert!(msg.contains("gddy payment-methods add"), "{msg}");
    }

    #[test]
    fn v3_error_envelope_fields_render_as_plain_english() {
        // v3 wraps validation detail under an `error` object; the friendly
        // renderer must reach into it.
        let body = r#"{"error":{"code":"INVALID_BODY","fields":[{"code":"LENGTH_UNDER","path":"contacts.registrant.address.line2"}]}}"#;
        let msg = format_api_error(
            "domain purchase",
            422,
            "422 Unprocessable Entity",
            body,
            None,
            false,
        );
        assert!(msg.contains("registrant address line 2"), "{msg}");
        assert!(msg.contains("is too short"), "{msg}");
        assert!(msg.contains("leave it blank to omit"), "{msg}");
    }

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

    #[test]
    fn v3_top_level_details_render_as_plain_english() {
        // The real body a DNS write returns on a name-exclusivity conflict: no
        // `fields`/`error` envelope at all, just a top-level `details[]`.
        let body = r#"{"correlationId":"abc-123","details":[{"description":"Duplicate data provided for record name, www.","issue":"DUPLICATE_RECORD"}],"message":"Request failed validation","name":"VALIDATION_ERROR"}"#;
        let msg = format_api_error(
            "dns set",
            422,
            "422 Unprocessable Entity",
            body,
            None,
            false,
        );
        assert!(
            msg.contains("Duplicate data provided for record name, www."),
            "{msg}"
        );
    }

    #[test]
    fn v3_details_with_no_description_fall_back_to_the_issue_code() {
        // `description` is documented as unstable and often omitted; `issue` is
        // the stable code. A detail with only `issue` must still render
        // something useful, not fall through to the generic opaque message.
        let body = r#"{"correlationId":"abc-123","details":[{"issue":"INVALID_NAMESERVER"}],"message":"Request failed validation","name":"VALIDATION_ERROR"}"#;
        let msg = format_api_error(
            "dns set",
            422,
            "422 Unprocessable Entity",
            body,
            None,
            false,
        );
        assert!(msg.contains("INVALID_NAMESERVER"), "{msg}");

        // Both empty → no friendly rendering, falls through to the generic message.
        let body = r#"{"correlationId":"abc-123","details":[{}],"message":"Request failed validation","name":"VALIDATION_ERROR"}"#;
        let msg = format_api_error(
            "dns set",
            422,
            "422 Unprocessable Entity",
            body,
            None,
            false,
        );
        assert!(!msg.contains("some fields are invalid"), "{msg}");
    }

    #[test]
    fn period_label_pluralizes_correctly() {
        assert_eq!(period_label(1), "1 year");
        assert_eq!(period_label(2), "2 years");
        assert_eq!(period_label(10), "10 years");
    }

    #[test]
    fn terminal_status_detection() {
        assert!(is_terminal_status("COMPLETED"));
        assert!(is_terminal_status("FAILED"));
        assert!(!is_terminal_status("CONFIRMED"));
        assert!(!is_terminal_status("EXECUTING"));
        assert!(!is_terminal_status("SUBMITTED"));
    }

    #[test]
    fn format_operation_error_includes_name_and_message() {
        assert_eq!(format_operation_error(None), "");

        let name_only = types::Error {
            name: Some("DOMAIN_UNAVAILABLE".to_string()),
            ..Default::default()
        };
        assert_eq!(
            format_operation_error(Some(&name_only)),
            " (DOMAIN_UNAVAILABLE)"
        );

        let message_only = types::Error {
            message: Some("the domain is already registered".to_string()),
            ..Default::default()
        };
        assert_eq!(
            format_operation_error(Some(&message_only)),
            " (the domain is already registered)"
        );

        let both = types::Error {
            name: Some("DOMAIN_UNAVAILABLE".to_string()),
            message: Some("the domain is already registered".to_string()),
            ..Default::default()
        };
        assert_eq!(
            format_operation_error(Some(&both)),
            " (DOMAIN_UNAVAILABLE: the domain is already registered)"
        );

        let neither = types::Error::default();
        assert_eq!(format_operation_error(Some(&neither)), "");
    }
}
