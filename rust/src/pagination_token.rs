//! Shared opaque `--continue` token codec.
//!
//! Most of this CLI's paginated commands are not backed by a real cursor
//! API — a fixed page/pageSize backend, or a fully in-memory collection —
//! so they synthesize their own continuation token instead of getting one
//! from a server. This module gives every one of them the same small
//! `key:value,key2:value2` shape (the proposal's "Instructions" token type)
//! instead of each hand-rolling its own parsing.
//!
//! A command whose backend already hands back a real opaque cursor (e.g.
//! `domain list`'s `pageToken`) does not need this — it just passes that
//! token straight through as [`cli_engine::CursorContinuation::more`]'s
//! argument.

use std::collections::HashMap;
use std::str::FromStr;

use cli_engine::CursorContinuation;
use serde_json::Value;

/// Parses a `key:value,key2:value2` token into a lookup map.
///
/// `None`/empty input parses to an empty map — an absent or blank
/// `--continue` always means "start from the beginning," never a parse
/// error. A malformed pair (no `:`) is silently skipped rather than
/// rejected, matching [`field`]'s "degrade to the default" philosophy: a
/// hand-edited or truncated `--continue` value should restart pagination,
/// not fail the command outright.
pub(crate) fn parse(token: Option<&str>) -> HashMap<String, String> {
    let Some(token) = token.filter(|t| !t.is_empty()) else {
        return HashMap::new();
    };
    token
        .split(',')
        .filter_map(|pair| pair.split_once(':'))
        .map(|(key, value)| (key.to_owned(), unescape(value)))
        .collect()
}

/// Escapes the two characters that would corrupt a `key:value,key:value`
/// token — `%` (the escape itself) and `,` (the pair separator) — so an opaque
/// value, such as a server's page token, survives [`build`] then [`parse`]
/// byte for byte.
fn escape(value: &str) -> String {
    value.replace('%', "%25").replace(',', "%2C")
}

/// Inverse of [`escape`]. `%2C` must be decoded before `%25`, so that an
/// original `%2C` (escaped to `%252C`) is not turned into a comma.
fn unescape(value: &str) -> String {
    value.replace("%2C", ",").replace("%25", "%")
}

/// Reads a typed field out of a [`parse`]d map, falling back to `default`
/// when the key is absent or its value doesn't parse as `T` — the same
/// "degrade to start over" behavior `parse` itself uses for a malformed
/// pair.
pub(crate) fn field<T: FromStr>(fields: &HashMap<String, String>, key: &str, default: T) -> T {
    fields
        .get(key)
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

/// Builds a `key:value,key2:value2` token from ordered pairs.
pub(crate) fn build(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(key, value)| format!("{key}:{}", escape(value)))
        .collect::<Vec<_>>()
        .join(",")
}

fn saturating_usize(value: i64) -> usize {
    usize::try_from(value).unwrap_or(usize::MAX)
}

fn saturating_i64(value: usize) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

/// Resolves the effective `--continue` value for a "known collection"
/// command. A `--continue` token (which includes a `limit`) is used as-is and
/// wins; otherwise an explicit `--offset` is synthesized into a bare
/// `offset:<n>` token (deliberately without a `limit` field — a fresh jump
/// has no prior page to inherit a size from, so [`paginate_known_collection`]
/// falls back to the request's own `--limit` when it sees one). Lets a user
/// jump straight to a position without walking there one token at a time,
/// without giving `--offset` and `--continue` two different encodings to
/// keep in sync.
///
/// The token has to win because the engine's "next page" action replays the
/// command's own arguments, so after `--offset 20` the advertised follow-up is
/// `… --offset 20 --continue <token>`; letting the offset override the token
/// would send the user back to the same page forever.
pub(crate) fn resolve_continue_token(
    explicit_offset: Option<i64>,
    continue_token: Option<&str>,
) -> Option<String> {
    continue_token
        .map(str::to_owned)
        .or_else(|| explicit_offset.map(|offset| build(&[("offset", &offset.to_string())])))
}

/// Resolves the starting page and page size for a page/pageSize-backed
/// command's request. Unlike an item-anchored token (e.g.
/// [`paginate_known_collection`]'s `offset:<n>`), "page N" is only
/// meaningful relative to the page size that produced it — resuming with
/// `--continue` but a *different* page size would silently request a
/// different slice than the one the token implies (page 2 of size-10 pages
/// covers different items than page 2 of size-25 pages). So a `--continue`
/// token's own embedded `limit` wins whenever present, letting a caller
/// resume with `--continue` alone and no need to repeat `--limit` — the same
/// way a real forward-only cursor already carries its own page size
/// server-side, opaque to the caller.
///
/// An explicit `--page` (a command's own supplemental direct-jump arg) is
/// used only when there is no `--continue` token, in which case `limit` (the
/// request's own parsed `--limit`) sets the size — there's no prior token to
/// inherit a size from. A token's page wins over `--page` for the same reason
/// as in [`resolve_continue_token`]: the engine's next-page action replays the
/// command's own `--page` alongside the new `--continue`.
pub(crate) fn resolve_page_request<P: FromStr>(
    explicit_page: Option<P>,
    continue_token: Option<&str>,
    default_page: P,
    limit: i64,
    max_limit: i64,
) -> (P, i64) {
    let fields = parse(continue_token);
    let token_page = fields.get("page").and_then(|value| value.parse().ok());
    let page = token_page.or(explicit_page).unwrap_or(default_page);
    let page_size = inherited_limit(&fields, limit, max_limit);
    (page, page_size)
}

/// The page size a `--continue` token carries, if it is within the command's
/// `1..=max_limit` range; otherwise the request's own already-validated
/// `--limit`. A hand-edited token must not widen a page past the command's
/// maximum (`limit:-1` would otherwise mean "everything") or send the backend
/// a size it rejects.
fn inherited_limit(fields: &HashMap<String, String>, request_limit: i64, max_limit: i64) -> i64 {
    let inherited = field(fields, "limit", request_limit);
    if (1..=max_limit).contains(&inherited) {
        inherited
    } else {
        request_limit
    }
}

/// Slices an already-fully-known collection using a `limit:<n>,offset:<n>`
/// continuation token.
pub(crate) fn paginate_known_collection(
    items: Vec<Value>,
    limit: i64,
    max_limit: i64,
    continue_token: Option<&str>,
) -> (Vec<Value>, CursorContinuation) {
    let fields = parse(continue_token);
    let offset = field(&fields, "offset", 0usize);
    let limit = inherited_limit(&fields, limit, max_limit);
    let limit = saturating_usize(limit).max(1);
    let total = items.len();
    let start = offset.min(total);
    let end = start.saturating_add(limit).min(total);
    let page = items.get(start..end).unwrap_or_default().to_vec();
    let remaining = total.saturating_sub(end);

    let mut continuation = if end < total {
        CursorContinuation::more(build(&[
            ("limit", &limit.to_string()),
            ("offset", &end.to_string()),
        ]))
    } else {
        CursorContinuation::done()
    };
    continuation = continuation
        .with_total(saturating_i64(total))
        .with_remaining(saturating_i64(remaining))
        .with_limit(saturating_i64(limit));
    (page, continuation)
}

/// Writes `message` as a one-line warning to `out`.
fn write_warning(out: &mut impl std::io::Write, message: &str) {
    let _ = writeln!(out, "warning: {message}");
}

/// Shows a non-fatal warning about an incomplete listing to the user on
/// stderr, keeping stdout's JSON/TOON payload clean.
///
/// Interim: the engine has no result-level warning channel yet. Replace calls
/// with `CommandResult::with_warning` once cli-engine#131 is released and
/// adopted.
pub(crate) fn warn_user(message: &str) {
    write_warning(&mut std::io::stderr(), message);
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn parse_reads_key_value_pairs() {
        let fields = parse(Some("page:2,limit:50"));
        assert_eq!(fields.get("page").map(String::as_str), Some("2"));
        assert_eq!(fields.get("limit").map(String::as_str), Some("50"));
    }

    #[test]
    fn parse_of_none_or_empty_is_an_empty_map() {
        assert!(parse(None).is_empty());
        assert!(parse(Some("")).is_empty());
    }

    #[test]
    fn parse_skips_a_pair_with_no_colon() {
        let fields = parse(Some("page:2,garbage,limit:50"));
        assert_eq!(fields.len(), 2);
    }

    #[test]
    fn field_falls_back_on_missing_or_unparseable_value() {
        let fields = parse(Some("page:not-a-number"));
        assert_eq!(field(&fields, "page", 1u32), 1);
        assert_eq!(field(&fields, "missing", 7u32), 7);
    }

    #[test]
    fn field_reads_a_present_valid_value() {
        let fields = parse(Some("page:3"));
        assert_eq!(field(&fields, "page", 1u32), 3);
    }

    #[test]
    fn build_and_parse_round_trip() {
        let token = build(&[("page", "3"), ("limit", "20")]);
        let fields = parse(Some(&token));
        assert_eq!(field(&fields, "page", 0u32), 3);
        assert_eq!(field(&fields, "limit", 0u32), 20);
    }

    #[test]
    fn resolve_continue_token_prefers_a_continue_token_over_an_explicit_offset() {
        // The engine's next-page action replays `--offset 20` next to the new
        // `--continue`; the token has to win or the user never advances.
        let resolved = resolve_continue_token(Some(20), Some("limit:5,offset:25"));
        assert_eq!(resolved.as_deref(), Some("limit:5,offset:25"));
    }

    #[test]
    fn resolve_continue_token_uses_an_explicit_offset_when_there_is_no_token() {
        let resolved = resolve_continue_token(Some(20), None);
        assert_eq!(resolved.as_deref(), Some("offset:20"));
    }

    #[test]
    fn resolve_continue_token_falls_back_to_the_continue_value() {
        let resolved = resolve_continue_token(None, Some("offset:5"));
        assert_eq!(resolved.as_deref(), Some("offset:5"));
    }

    #[test]
    fn resolve_continue_token_of_neither_is_none() {
        assert_eq!(resolve_continue_token(None, None), None);
    }

    #[test]
    fn resolve_page_request_with_no_input_defaults_the_page_and_uses_the_given_limit() {
        let (page, page_size) = resolve_page_request(None::<u32>, None, 1u32, 25, 100);
        assert_eq!(page, 1);
        assert_eq!(page_size, 25);
    }

    #[test]
    fn resolve_page_request_inherits_both_page_and_size_from_the_continue_token() {
        // The size that produced page 2 (10) must carry forward even though
        // this call's own --limit defaulted to something else (25) — page 2
        // of size-10 pages and page 2 of size-25 pages cover different items.
        let token = build(&[("limit", "10"), ("page", "2")]);
        let (page, page_size) = resolve_page_request(None::<u32>, Some(&token), 1u32, 25, 100);
        assert_eq!(page, 2);
        assert_eq!(page_size, 10);
    }

    #[test]
    fn resolve_page_request_an_explicit_page_is_used_without_a_token_and_takes_the_requests_limit()
    {
        // No continue token, so there's no prior size to inherit — the
        // request's own limit applies.
        let (page, page_size) = resolve_page_request(Some(5u32), None, 1u32, 25, 100);
        assert_eq!(page, 5);
        assert_eq!(page_size, 25);
    }

    #[test]
    fn resolve_page_request_a_continue_token_wins_over_an_explicit_page() {
        // The engine's next-page action replays `--page 5` next to the new
        // `--continue`; the token's page has to win or the user never advances.
        let token = build(&[("limit", "10"), ("page", "6")]);
        let (page, page_size) = resolve_page_request(Some(5u32), Some(&token), 1u32, 25, 100);
        assert_eq!(page, 6);
        assert_eq!(page_size, 10);
    }

    fn items(n: usize) -> Vec<Value> {
        (0..n).map(|i| json!({ "id": i })).collect()
    }

    #[test]
    fn paginate_known_collection_returns_the_first_page_with_no_token() {
        let (page, cont) = paginate_known_collection(items(5), 2, 100, None);
        assert_eq!(page, vec![json!({"id": 0}), json!({"id": 1})]);
        assert!(cont.continue_from.is_some());
        assert_eq!(cont.total, Some(5));
        assert_eq!(cont.remaining, Some(3));
    }

    #[test]
    fn paginate_known_collection_resumes_from_the_continue_token() {
        let (_, cont) = paginate_known_collection(items(5), 2, 100, None);
        let token = cont.continue_from.expect("has_more implies a token");
        let (page, cont2) = paginate_known_collection(items(5), 2, 100, Some(&token));
        assert_eq!(page, vec![json!({"id": 2}), json!({"id": 3})]);
        assert!(cont2.continue_from.is_some());
        assert_eq!(cont2.remaining, Some(1));
    }

    #[test]
    fn paginate_known_collection_continue_token_embeds_and_inherits_the_limit() {
        let (_, cont) = paginate_known_collection(items(5), 2, 100, None);
        let token = cont.continue_from.expect("has_more implies a token");
        assert_eq!(token, "limit:2,offset:2");

        // A caller that resumes with --continue alone, and whose --limit
        // defaulted to something else entirely (25) this call, must still
        // get the size (2) that produced the token, not the new default.
        let (page, cont2) = paginate_known_collection(items(5), 25, 100, Some(&token));
        assert_eq!(page, vec![json!({"id": 2}), json!({"id": 3})]);
        assert_eq!(cont2.remaining, Some(1));
    }

    #[test]
    fn paginate_known_collection_an_explicit_offset_token_has_no_limit_to_inherit() {
        // resolve_continue_token synthesizes a bare `offset:<n>` (no
        // `limit`) for an explicit --offset — the request's own limit
        // applies, same as a fresh, tokenless call.
        let token = resolve_continue_token(Some(2), None).expect("explicit offset");
        let (page, _) = paginate_known_collection(items(5), 3, 100, Some(&token));
        assert_eq!(
            page,
            vec![json!({"id": 2}), json!({"id": 3}), json!({"id": 4})]
        );
    }

    #[test]
    fn paginate_known_collection_reaches_the_end_with_no_further_token() {
        let (page, cont) = paginate_known_collection(items(5), 2, 100, Some("offset:4"));
        assert_eq!(page, vec![json!({"id": 4})]);
        assert!(cont.continue_from.is_none());
        assert_eq!(cont.remaining, Some(0));
    }

    #[test]
    fn paginate_known_collection_past_the_end_returns_empty() {
        let (page, cont) = paginate_known_collection(items(3), 2, 100, Some("offset:10"));
        assert!(page.is_empty());
        assert!(cont.continue_from.is_none());
        assert_eq!(cont.remaining, Some(0));
    }

    #[test]
    fn paginate_known_collection_of_an_empty_collection() {
        let (page, cont) = paginate_known_collection(Vec::new(), 10, 100, None);
        assert!(page.is_empty());
        assert!(cont.continue_from.is_none());
        assert_eq!(cont.total, Some(0));
    }

    #[test]
    fn write_warning_emits_one_prefixed_line() {
        let mut out = Vec::new();
        write_warning(&mut out, "list may be incomplete");
        assert_eq!(
            String::from_utf8(out).expect("utf8"),
            "warning: list may be incomplete\n"
        );
    }

    #[test]
    fn build_and_parse_round_trip_opaque_values_byte_for_byte() {
        for opaque in [
            "abc,def", "100%", "a+b c", "%2C", "%252C", ",,%,%25,", "x:y,z",
        ] {
            let token = build(&[("token", opaque), ("direction", "forward")]);
            let fields = parse(Some(&token));
            assert_eq!(
                fields.get("token").map(String::as_str),
                Some(opaque),
                "{token}"
            );
            assert_eq!(fields.get("direction").map(String::as_str), Some("forward"));
        }
    }

    #[test]
    fn a_comma_in_a_value_does_not_split_the_token() {
        assert_eq!(build(&[("token", "abc,def")]), "token:abc%2Cdef");
    }

    #[test]
    fn paginate_known_collection_ignores_an_out_of_range_token_limit() {
        // Negative would otherwise convert to usize::MAX and return everything;
        // zero and oversized values fall back to the request's own limit.
        for bad in [
            "limit:-1,offset:0",
            "limit:0,offset:0",
            "limit:101,offset:0",
        ] {
            let (page, _) = paginate_known_collection(items(300), 7, 100, Some(bad));
            assert_eq!(page.len(), 7, "{bad}");
        }
        let (page, _) = paginate_known_collection(items(300), 7, 100, Some("limit:100,offset:0"));
        assert_eq!(
            page.len(),
            100,
            "an in-range token limit is still inherited"
        );
    }

    #[test]
    fn resolve_page_request_ignores_an_out_of_range_token_limit() {
        for bad in ["limit:-1,page:2", "limit:0,page:2", "limit:101,page:2"] {
            let (page, page_size) = resolve_page_request(None::<u32>, Some(bad), 1u32, 25, 100);
            assert_eq!((page, page_size), (2, 25), "{bad}");
        }
        let (_, page_size) =
            resolve_page_request(None::<u32>, Some("limit:100,page:2"), 1u32, 25, 100);
        assert_eq!(page_size, 100);
    }
}
