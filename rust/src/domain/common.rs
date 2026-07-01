//! Shared helpers for the `domain` command group: the authenticated Domains API
//! client, money formatting, argument helpers, and API-error rendering. Each
//! `gddy domain` subcommand lives in its own sibling module and draws from here.

use cli_engine::{CliCoreError, CommandContext, Credential, Result};

use crate::{auth::SSO_KEY_PROVIDER, environments};

use domains_client::types;

const USER_AGENT: &str = concat!("godaddy-cli/", env!("CARGO_PKG_VERSION"));

fn map_env_err(e: environments::EnvError) -> CliCoreError {
    CliCoreError::message(e.to_string())
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

/// Pick the `Authorization` header value for a resolved credential: the `sso-key`
/// scheme for the [`SSO_KEY_PROVIDER`] bypass path, otherwise an OAuth `Bearer`
/// token. Pure so the scheme selection is unit-testable without a full context.
fn authorization_header(provider: &str, token: &str) -> String {
    if provider == SSO_KEY_PROVIDER {
        format!("sso-key {token}")
    } else {
        format!("Bearer {token}")
    }
}

/// Build a Domains API client for the active environment, choosing the auth
/// scheme from the resolved credential (sso-key for the bypass path, else Bearer).
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
    let domains = environments::resolve_domains(env).map_err(map_env_err)?;
    let authorization = authorization_header(&cred.provider, &cred.token);
    let request_id = uuid::Uuid::new_v4().to_string();
    domains_client::client_with_auth(&domains.base_url, &authorization, USER_AGENT, &request_id)
        .map_err(|e| CliCoreError::message(format!("failed to build domains client: {e}")))
}

/// Collect a repeatable string argument into a `Vec` (clap stores it as a JSON
/// array; a lone value arrives as a string).
pub(crate) fn string_list(ctx: &CommandContext, key: &str) -> Vec<String> {
    match ctx.args.get(key) {
        Some(serde_json::Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect(),
        Some(serde_json::Value::String(s)) => vec![s.clone()],
        _ => Vec::new(),
    }
}

/// Turn a domains-client error into a `CliCoreError`, reading the response body
/// for unexpected (non-2xx) responses so the API's actual message isn't lost
/// (progenitor's `Display` prints only the status). Async because reading the
/// body is async.
pub(super) async fn api_error(
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
fn format_api_error(
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
             `gddy payments add` (a credit card or Good-as-Gold balance is required for domain \
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

/// A Domains API validation error body. Tolerates both the v1 shape
/// (`{"fields":[...]}`) and the v3 shape (`{"error":{"fields":[...]}}`).
#[derive(serde::Deserialize)]
struct ApiErrorBody {
    #[serde(default)]
    fields: Vec<ApiFieldError>,
    #[serde(default)]
    error: Option<ApiErrorEnvelope>,
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

/// Render a structured validation error as a plain-English bullet list, or `None`
/// when `body` isn't a field-level validation error.
fn friendly_field_errors(body: &str) -> Option<String> {
    let parsed = serde_json::from_str::<ApiErrorBody>(body).ok()?;
    let fields = if !parsed.fields.is_empty() {
        parsed.fields
    } else {
        parsed.error.map(|e| e.fields).unwrap_or_default()
    };
    if fields.is_empty() {
        return None;
    }
    let lines: Vec<String> = fields
        .iter()
        .map(|f| format!("  • {}", describe_field_error(f)))
        .collect();
    Some(format!("some fields are invalid:\n{}", lines.join("\n")))
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
    fn authorization_header_picks_scheme_from_provider() {
        assert_eq!(
            authorization_header(SSO_KEY_PROVIDER, "KEY:SECRET"),
            "sso-key KEY:SECRET"
        );
        assert_eq!(authorization_header("godaddy", "tok123"), "Bearer tok123");
    }

    #[test]
    fn payment_required_error_points_to_payments_add() {
        let msg = format_api_error(
            "domain purchase",
            402,
            "402 Payment Required",
            r#"{"code":"INVALID_PAYMENT_INFO","message":"Unable to authorize credit"}"#,
            None,
            false,
        );
        assert!(msg.contains("402 Payment Required"), "{msg}");
        assert!(msg.contains("gddy payments add"), "{msg}");
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
}
