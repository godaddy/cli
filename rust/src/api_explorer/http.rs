//! Small, pure helpers used only by `api call` (see `super::call`): header
//! parsing, mutating-method detection for `--dry-run` gating, response-body
//! parsing, GraphQL error extraction, and OAuth scope merging.

use serde_json::{Value, json};

/// Split a `--header` value of the form `KEY:VALUE` into trimmed parts.
/// Splits on the first colon only, so values may themselves contain colons
/// (e.g. a URL). Returns None when there is no colon.
pub(super) fn split_header(raw: &str) -> Option<(&str, &str)> {
    raw.split_once(':').map(|(k, v)| (k.trim(), v.trim()))
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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        graphql_errors, is_mutating_method, merge_required_scopes, parse_response_body,
        split_header,
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
}
