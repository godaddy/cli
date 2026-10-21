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
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect()
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
        .map(|(key, value)| format!("{key}:{value}"))
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
/// command: an explicit `--offset` wins, synthesized into a bare
/// `offset:<n>` token (deliberately without a `limit` field — a fresh jump
/// has no prior page to inherit a size from, so [`paginate_known_collection`]
/// falls back to the request's own `--limit` when it sees one); otherwise
/// the parsed `--continue` token (which includes a `limit`) is used as-is. Lets
/// a user jump straight to a position without walking there one token at a
/// time, without giving `--offset` and `--continue` two different encodings to
/// keep in sync.
pub(crate) fn resolve_continue_token(
    explicit_offset: Option<i64>,
    continue_token: Option<&str>,
) -> Option<String> {
    explicit_offset
        .map(|offset| build(&[("offset", &offset.to_string())]))
        .or_else(|| continue_token.map(str::to_owned))
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
/// An explicit `--page` (a command's own supplemental direct-jump arg,
/// `conflicts_with = "continue"`) always wins for the page instead, and in
/// that case `limit` (the request's own parsed `--limit`) always wins for
/// the size — there's no prior token to inherit a size from.
pub(crate) fn resolve_page_request<P: FromStr>(
    explicit_page: Option<P>,
    continue_token: Option<&str>,
    default_page: P,
    limit: i64,
) -> (P, i64) {
    let fields = parse(continue_token);
    let page = explicit_page.unwrap_or_else(|| field(&fields, "page", default_page));
    let page_size = field(&fields, "limit", limit);
    (page, page_size)
}

/// Slices an already-fully-known collection using a `limit:<n>,offset:<n>`
/// continuation token.
pub(crate) fn paginate_known_collection(
    items: Vec<Value>,
    limit: i64,
    continue_token: Option<&str>,
) -> (Vec<Value>, CursorContinuation) {
    let fields = parse(continue_token);
    let offset = field(&fields, "offset", 0usize);
    let limit = field(&fields, "limit", limit);
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
    fn resolve_continue_token_prefers_an_explicit_offset() {
        let resolved = resolve_continue_token(Some(20), Some("offset:5"));
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
        let (page, page_size) = resolve_page_request(None::<u32>, None, 1u32, 25);
        assert_eq!(page, 1);
        assert_eq!(page_size, 25);
    }

    #[test]
    fn resolve_page_request_inherits_both_page_and_size_from_the_continue_token() {
        // The size that produced page 2 (10) must carry forward even though
        // this call's own --limit defaulted to something else (25) — page 2
        // of size-10 pages and page 2 of size-25 pages cover different items.
        let token = build(&[("limit", "10"), ("page", "2")]);
        let (page, page_size) = resolve_page_request(None::<u32>, Some(&token), 1u32, 25);
        assert_eq!(page, 2);
        assert_eq!(page_size, 10);
    }

    #[test]
    fn resolve_page_request_an_explicit_page_wins_and_uses_the_requests_own_limit() {
        // No continue token at all (--page conflicts_with --continue), so
        // there's no prior size to inherit — the request's own limit applies.
        let (page, page_size) = resolve_page_request(Some(5u32), None, 1u32, 25);
        assert_eq!(page, 5);
        assert_eq!(page_size, 25);
    }

    fn items(n: usize) -> Vec<Value> {
        (0..n).map(|i| json!({ "id": i })).collect()
    }

    #[test]
    fn paginate_known_collection_returns_the_first_page_with_no_token() {
        let (page, cont) = paginate_known_collection(items(5), 2, None);
        assert_eq!(page, vec![json!({"id": 0}), json!({"id": 1})]);
        assert!(cont.continue_from.is_some());
        assert_eq!(cont.total, Some(5));
        assert_eq!(cont.remaining, Some(3));
    }

    #[test]
    fn paginate_known_collection_resumes_from_the_continue_token() {
        let (_, cont) = paginate_known_collection(items(5), 2, None);
        let token = cont.continue_from.expect("has_more implies a token");
        let (page, cont2) = paginate_known_collection(items(5), 2, Some(&token));
        assert_eq!(page, vec![json!({"id": 2}), json!({"id": 3})]);
        assert!(cont2.continue_from.is_some());
        assert_eq!(cont2.remaining, Some(1));
    }

    #[test]
    fn paginate_known_collection_continue_token_embeds_and_inherits_the_limit() {
        let (_, cont) = paginate_known_collection(items(5), 2, None);
        let token = cont.continue_from.expect("has_more implies a token");
        assert_eq!(token, "limit:2,offset:2");

        // A caller that resumes with --continue alone, and whose --limit
        // defaulted to something else entirely (25) this call, must still
        // get the size (2) that produced the token, not the new default.
        let (page, cont2) = paginate_known_collection(items(5), 25, Some(&token));
        assert_eq!(page, vec![json!({"id": 2}), json!({"id": 3})]);
        assert_eq!(cont2.remaining, Some(1));
    }

    #[test]
    fn paginate_known_collection_an_explicit_offset_token_has_no_limit_to_inherit() {
        // resolve_continue_token synthesizes a bare `offset:<n>` (no
        // `limit`) for an explicit --offset — the request's own limit
        // applies, same as a fresh, tokenless call.
        let token = resolve_continue_token(Some(2), None).expect("explicit offset");
        let (page, _) = paginate_known_collection(items(5), 3, Some(&token));
        assert_eq!(
            page,
            vec![json!({"id": 2}), json!({"id": 3}), json!({"id": 4})]
        );
    }

    #[test]
    fn paginate_known_collection_reaches_the_end_with_no_further_token() {
        let (page, cont) = paginate_known_collection(items(5), 2, Some("offset:4"));
        assert_eq!(page, vec![json!({"id": 4})]);
        assert!(cont.continue_from.is_none());
        assert_eq!(cont.remaining, Some(0));
    }

    #[test]
    fn paginate_known_collection_past_the_end_returns_empty() {
        let (page, cont) = paginate_known_collection(items(3), 2, Some("offset:10"));
        assert!(page.is_empty());
        assert!(cont.continue_from.is_none());
        assert_eq!(cont.remaining, Some(0));
    }

    #[test]
    fn paginate_known_collection_of_an_empty_collection() {
        let (page, cont) = paginate_known_collection(Vec::new(), 10, None);
        assert!(page.is_empty());
        assert!(cont.continue_from.is_none());
        assert_eq!(cont.total, Some(0));
    }
}
