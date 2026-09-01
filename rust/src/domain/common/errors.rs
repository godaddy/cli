use cli_engine::CliCoreError;

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

}
