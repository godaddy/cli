//! Small, pure helpers plus the shared HTTP transport tail used by `api
//! call` (see `super::call`) and `api graphql call` (see
//! `super::graphql::call`): header parsing, mutating-method detection for
//! `--dry-run` gating, response-body parsing, GraphQL error extraction, and
//! OAuth scope merging.

use cli_engine::CommandResult;
use serde_json::{Map, Value, json};

/// Split a `--header` value of the form `KEY:VALUE` into trimmed parts.
/// Splits on the first colon only, so values may themselves contain colons
/// (e.g. a URL). Returns None when there is no colon or the key is empty
/// (e.g. `":value"`) — reqwest rejects an empty header name with a less
/// actionable "invalid header name" error than the caller's own
/// `expected KEY:VALUE` validation.
pub(super) fn split_header(raw: &str) -> Option<(&str, &str)> {
    let (k, v) = raw.split_once(':')?;
    let k = k.trim();
    (!k.is_empty()).then(|| (k, v.trim()))
}

/// Whether an HTTP method mutates server state, for `--dry-run` gating.
/// `call`'s tier is fixed at spec-build time, but the method is a runtime
/// arg — this decides per-invocation whether `--dry-run` should actually
/// short-circuit the request. Case-insensitive.
pub(super) fn is_mutating_method(method: &str) -> bool {
    !(method.eq_ignore_ascii_case("GET") || method.eq_ignore_ascii_case("HEAD"))
}

/// Parse a response body as JSON when possible, otherwise preserve it as raw
/// UTF-8 text. A non-JSON body (plain text / HTML error page) must not be
/// silently dropped to `null` — only a truly empty or binary body becomes null.
pub(super) fn parse_response_body(bytes: &[u8]) -> Value {
    match serde_json::from_slice::<Value>(bytes) {
        Ok(v) => v,
        Err(_) => match std::str::from_utf8(bytes) {
            Ok(s) if !s.is_empty() => json!(s),
            _ => Value::Null,
        },
    }
}

/// A non-empty top-level GraphQL `errors` array, if present. GraphQL endpoints
/// return HTTP 200 even on failure and carry the failure here.
pub(super) fn graphql_errors(body: &Value) -> Option<&Vec<Value>> {
    body.get("errors")
        .and_then(|e| e.as_array())
        .filter(|a| !a.is_empty())
}

/// Union of user-supplied `--scope` flags and a matched endpoint's declared
/// scopes, order-preserving and de-duplicated (flags first).
pub(super) fn merge_required_scopes(
    flag_scopes: Vec<String>,
    endpoint_scopes: &[String],
) -> Vec<String> {
    let mut required: Vec<String> = Vec::new();
    // De-dup across both sources (a user can repeat `--scope`), flags first.
    for scope in flag_scopes
        .into_iter()
        .chain(endpoint_scopes.iter().cloned())
    {
        if !required.contains(&scope) {
            required.push(scope);
        }
    }
    required
}

/// Parses repeatable `--header KEY:VALUE` flags into a validated list.
pub(super) fn parsed_extra_headers(
    raw: &[String],
) -> Result<Vec<(String, String)>, cli_engine::CliCoreError> {
    raw.iter()
        .map(|h| {
            split_header(h)
                .map(|(k, v)| (k.to_owned(), v.to_owned()))
                .ok_or_else(|| {
                    crate::error::GddyError::validation(format!(
                        "invalid header '{h}': expected KEY:VALUE"
                    ))
                    .into_cli_error()
                })
        })
        .collect()
}

/// Sends a fully-built request and reports the result as a `CommandResult`
/// — shared by `call_command`'s raw-path flow and
/// `graphql::call::call_command`'s synthesized flow, since both need the
/// same status/GraphQL-error/403/non-2xx handling once the request itself
/// differs (raw path + user body vs. synthesized GraphQL query + variables).
#[allow(clippy::too_many_arguments)]
pub(super) async fn send_and_report(
    client: &reqwest::Client,
    parsed_method: reqwest::Method,
    method: &str,
    url: &str,
    token: &str,
    extra_headers: &[(String, String)],
    request_body: Option<Value>,
    include_headers: bool,
    is_graphql: bool,
    required: &[String],
    endpoint: &str,
) -> Result<CommandResult, cli_engine::CliCoreError> {
    let mut req = client
        .request(parsed_method, url)
        .bearer_auth(token)
        .header("x-request-id", uuid::Uuid::new_v4().to_string());

    for (key, val) in extra_headers {
        req = req.header(key, val);
    }

    if let Some(body) = request_body {
        req = req.json(&body);
    }

    let request = req
        .build()
        .map_err(|e| crate::error::GddyError::validation(e.to_string()))?;
    cli_engine::transport::debug_log_reqwest_request(&request);
    let resp = client
        .execute(request)
        .await
        .map_err(|e| crate::error::GddyError::network(e.to_string()))?;

    let status_code = resp.status();
    let status_text = status_code.canonical_reason().unwrap_or("").to_owned();
    let response_headers_raw = resp.headers().clone();
    let body_bytes = resp
        .bytes()
        .await
        .map_err(|e| crate::error::GddyError::network(e.to_string()))?;
    cli_engine::transport::debug_log_reqwest_response(
        status_code,
        &response_headers_raw,
        &body_bytes,
    );

    let status = status_code.as_u16();
    let response_headers: Option<Map<String, Value>> = if include_headers {
        Some(
            response_headers_raw
                .iter()
                .filter_map(|(k, v)| v.to_str().ok().map(|s| (k.to_string(), json!(s))))
                .collect(),
        )
    } else {
        None
    };

    let body: Value = parse_response_body(&body_bytes);

    // GraphQL endpoints return HTTP 200 even on failure, carrying the error
    // in a top-level `errors` array.
    if is_graphql && let Some(errors) = graphql_errors(&body) {
        return Err(crate::error::GddyError::from_graphql(
            format!(
                "GraphQL request returned {} error(s):\n{}",
                errors.len(),
                serde_json::to_string_pretty(&json!(errors)).unwrap_or_default(),
            ),
            "api",
        )
        .into());
    }

    // Scope step-up already ran up front (the token was requested with
    // `required`). A 403 here means the granted token still lacks a
    // required scope — surface it with a re-login hint.
    if status == 403 && !required.is_empty() {
        // `auth login --scope` is append-style (one value per flag), so
        // repeat the flag rather than space-joining.
        let login_hint = required
            .iter()
            .map(|s| format!("--scope {s}"))
            .collect::<Vec<_>>()
            .join(" ");
        return Err(crate::error::GddyError::auth(format!(
            "403 Forbidden — the authorized token is missing required scope(s): {}. \
             Re-run `gddy auth login {login_hint}` and try again.",
            required.join(", "),
        ))
        .with_fix(format!("Run: gddy auth login {login_hint}"))
        .into());
    }

    // Any other non-2xx is a failure, not a success envelope. Include the
    // status line and (truncated) response body so the caller sees the
    // detail instead of a success result that happens to carry an error
    // payload.
    if !(200..300).contains(&status) {
        let detail: String = serde_json::to_string_pretty(&body)
            .unwrap_or_else(|_| body.to_string())
            .chars()
            .take(4000)
            .collect();
        return Err(crate::error::GddyError::from_http(
            status,
            format!("{status_text}\n{detail}"),
            "api",
        )
        .into_cli_error());
    }

    // Identify the call and its outcome in the result envelope.
    let mut result = json!({
        "endpoint": endpoint,
        "method": method,
        "status": status,
        "status_text": status_text,
        "data": body,
    });
    if let Some(headers) = response_headers {
        result["headers"] = Value::Object(headers);
    }

    Ok(CommandResult::new(result))
}

#[cfg(test)]
mod tests {
    use httpmock::MockServer;
    use serde_json::json;

    use super::{
        graphql_errors, is_mutating_method, merge_required_scopes, parse_response_body,
        send_and_report, split_header,
    };
    use crate::api_explorer::catalog::{catalog, find_endpoint};

    fn v(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn merge_flags_only_when_no_endpoint_scopes() {
        assert_eq!(merge_required_scopes(v(&["a", "b"]), &[]), v(&["a", "b"]));
    }

    #[test]
    fn merge_endpoint_only_when_no_flags() {
        assert_eq!(
            merge_required_scopes(v(&[]), &v(&["x", "y"])),
            v(&["x", "y"])
        );
    }

    #[test]
    fn merge_unions_and_dedupes_flags_first() {
        assert_eq!(
            merge_required_scopes(v(&["a", "b"]), &v(&["b", "c"])),
            v(&["a", "b", "c"])
        );
    }

    #[test]
    fn merge_dedupes_repeated_flag_values() {
        assert_eq!(
            merge_required_scopes(v(&["a", "a", "b"]), &v(&["b"])),
            v(&["a", "b"])
        );
    }

    /// The commerce endpoint's catalog scope is included alongside an explicit
    /// scope. `call_command` sends this exact list to
    /// `credential_with_scopes` before it builds the HTTP request, so the PKCE
    /// provider can perform OAuth step-up before an API 403.
    #[test]
    fn commerce_catalog_scope_is_merged_with_an_explicit_scope() {
        let (_, endpoint) = find_endpoint(catalog(), "/v1/commerce/stores/{storeId}/orders")
            .expect("orders endpoint exists in the embedded catalog");
        assert_eq!(
            merge_required_scopes(v(&["commerce.order:write"]), &endpoint.scopes),
            v(&["commerce.order:write", "commerce.order:read"]),
        );
    }

    #[test]
    fn split_header_trims_and_splits_on_first_colon() {
        assert_eq!(
            split_header("x-store-id: abc-123"),
            Some(("x-store-id", "abc-123"))
        );
        // Value may contain colons (e.g. a URL) — only the first colon splits.
        assert_eq!(
            split_header("Location: https://x/y"),
            Some(("Location", "https://x/y"))
        );
        assert_eq!(split_header("no-colon"), None);
    }

    #[test]
    fn split_header_rejects_an_empty_key() {
        assert_eq!(split_header(":value"), None);
        assert_eq!(split_header("  :value"), None);
    }

    #[test]
    fn parse_response_body_json_text_and_empty() {
        assert_eq!(parse_response_body(b"{\"a\":1}"), json!({"a":1}));
        // Non-JSON is preserved as text, not dropped to null.
        assert_eq!(parse_response_body(b"plain text"), json!("plain text"));
        // Empty body is null.
        assert_eq!(parse_response_body(b""), serde_json::Value::Null);
    }

    #[test]
    fn graphql_errors_detects_nonempty_array_only() {
        assert!(graphql_errors(&json!({"data": null, "errors": [{"message": "x"}]})).is_some());
        assert!(graphql_errors(&json!({"data": {}, "errors": []})).is_none());
        assert!(graphql_errors(&json!({"data": {}})).is_none());
    }

    #[test]
    fn is_mutating_method_treats_only_get_and_head_as_safe() {
        assert!(!is_mutating_method("GET"));
        assert!(!is_mutating_method("get"));
        assert!(!is_mutating_method("HEAD"));
        assert!(is_mutating_method("POST"));
        assert!(is_mutating_method("PUT"));
        assert!(is_mutating_method("PATCH"));
        assert!(is_mutating_method("DELETE"));
    }

    #[tokio::test]
    async fn send_and_report_surfaces_graphql_errors_from_a_200_response() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST).path("/graphql");
                then.status(200)
                    .json_body(json!({"data": null, "errors": [{"message": "boom"}]}));
            })
            .await;

        let err = send_and_report(
            &reqwest::Client::new(),
            reqwest::Method::POST,
            "POST",
            &format!("{}/graphql", server.base_url()),
            "test-token",
            &[],
            None,
            false,
            true,
            &[],
            "postTaxGraphql",
        )
        .await
        .expect_err("a top-level GraphQL errors array must fail even on HTTP 200");

        assert!(err.to_string().contains("GraphQL request returned 1 error"));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn send_and_report_reports_missing_scopes_on_403() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::GET).path("/thing");
                then.status(403);
            })
            .await;

        let err = send_and_report(
            &reqwest::Client::new(),
            reqwest::Method::GET,
            "GET",
            &format!("{}/thing", server.base_url()),
            "test-token",
            &[],
            None,
            false,
            false,
            &v(&["commerce.order:write"]),
            "getThing",
        )
        .await
        .expect_err("403 with a required scope must fail with a re-login hint");

        let msg = err.to_string();
        assert!(msg.contains("403 Forbidden"));
        assert!(msg.contains("commerce.order:write"));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn send_and_report_truncates_a_long_non_2xx_body() {
        let server = MockServer::start_async().await;
        let long_detail = "x".repeat(10_000);
        let mock = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::GET).path("/thing");
                then.status(500).json_body(json!({"detail": long_detail}));
            })
            .await;

        let err = send_and_report(
            &reqwest::Client::new(),
            reqwest::Method::GET,
            "GET",
            &format!("{}/thing", server.base_url()),
            "test-token",
            &[],
            None,
            false,
            false,
            &[],
            "getThing",
        )
        .await
        .expect_err("a non-2xx status must fail");

        let msg = err.to_string();
        assert!(msg.len() < 5_000, "expected the body to be truncated");
        assert!(!msg.contains(&"x".repeat(10_000)));
        mock.assert_async().await;
    }
}
