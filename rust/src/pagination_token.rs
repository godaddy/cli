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

use cli_engine::{CliCoreError, CursorContinuation, PipelineOpts, apply_pipeline};
use serde_json::Value;

/// The validated `key:value` pairs of a `--continue` token.
#[derive(Debug)]
pub(crate) struct Fields {
    raw: String,
    map: HashMap<String, String>,
}

impl Fields {
    fn empty() -> Self {
        Self {
            raw: String::new(),
            map: HashMap::new(),
        }
    }

    /// The raw text of a field, if the token has it.
    pub(crate) fn text(&self, key: &str) -> Option<&str> {
        self.map.get(key).map(String::as_str)
    }

    /// A field parsed as `T`: `Ok(None)` when absent, an error when present but
    /// not a valid `T`.
    pub(crate) fn typed<T: FromStr>(&self, key: &str) -> cli_engine::Result<Option<T>> {
        match self.map.get(key) {
            None => Ok(None),
            Some(value) => value
                .parse()
                .map(Some)
                .map_err(|_| invalid_token(&self.raw, format!("`{key}` has an invalid value"))),
        }
    }
}

/// The error for a `--continue` value that is not a token this command issued.
/// A mistyped or truncated token must not quietly restart from the first page,
/// where an agent could loop over duplicate pages without noticing.
pub(crate) fn invalid_token(raw: &str, reason: impl std::fmt::Display) -> CliCoreError {
    let shown: String = raw.chars().take(80).collect();
    crate::error::GddyError::validation(format!("invalid --continue value \"{shown}\": {reason}"))
        .with_fix(
            "Use the --continue value from the previous page's next action exactly as printed, \
             or leave --continue out to start from the beginning.",
        )
        .into_cli_error()
}

/// Parses a `key:value,key2:value2` token, accepting only the `allowed` keys.
///
/// An absent or blank token is "start from the beginning" and parses to no
/// fields. Anything else must be well-formed: a pair with no `:`, or a key this
/// command never issues (for example a token pasted from another command), is
/// rejected rather than guessed at.
pub(crate) fn parse(token: Option<&str>, allowed: &[&str]) -> cli_engine::Result<Fields> {
    let Some(raw) = token.filter(|t| !t.trim().is_empty()) else {
        return Ok(Fields::empty());
    };
    let mut map = HashMap::new();
    for pair in raw.split(',') {
        let Some((key, value)) = pair.split_once(':') else {
            return Err(invalid_token(
                raw,
                "expected comma-separated key:value pairs",
            ));
        };
        if !allowed.contains(&key) {
            return Err(invalid_token(
                raw,
                format!("`{key}` is not part of this command's tokens"),
            ));
        }
        map.insert(key.to_owned(), unescape(value));
    }
    Ok(Fields {
        raw: raw.to_owned(),
        map,
    })
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
) -> cli_engine::Result<(P, i64)> {
    let fields = parse(continue_token, &["limit", "page"])?;
    let page = match fields.typed::<P>("page")? {
        Some(page) => {
            // Pages are 1-based, and a token is only ever minted as `page + 1`.
            // Zero would be clamped to page one by a caller that cannot send
            // it, then advertise page one again as the next page.
            if fields.typed::<u64>("page")? == Some(0) {
                return Err(invalid_token(&fields.raw, "`page` must be at least 1"));
            }
            page
        }
        None => explicit_page.unwrap_or(default_page),
    };
    let page_size = limit_from(&fields, limit, max_limit)?;
    Ok((page, page_size))
}

/// The page size a `--continue` token carries, which must be within the
/// command's `1..=max_limit`; without one, the request's own already-validated
/// `--limit`. A token cannot widen a page past the command's maximum.
fn limit_from(fields: &Fields, request_limit: i64, max_limit: i64) -> cli_engine::Result<i64> {
    match fields.typed::<i64>("limit")? {
        None => Ok(request_limit),
        Some(limit) if (1..=max_limit).contains(&limit) => Ok(limit),
        Some(limit) => Err(invalid_token(
            &fields.raw,
            format!("`limit` {limit} must be between 1 and {max_limit}"),
        )),
    }
}

/// Applies the global `--filter` to an already-fully-known collection
/// *before* it is sliced.
///
/// The engine runs `--filter` after the handler returns, so for a cursor
/// command it would filter the page it was handed — pages of a filtered
/// collection would come out short, offsets would select different rows than
/// they did when the engine paginated after filtering, and `cursor.total`
/// would count unfiltered rows. A command that holds the whole collection
/// filters it here first, with the engine's own filter, and propagates its
/// invalid-expression error. The engine's later pass over the returned page is
/// harmless: a per-item predicate keeps what it already kept.
pub(crate) fn filter_known_collection(
    items: Vec<Value>,
    filter: &str,
) -> cli_engine::Result<Vec<Value>> {
    if filter.is_empty() {
        return Ok(items);
    }
    let mut data = Value::Array(items);
    apply_pipeline(
        &mut data,
        &PipelineOpts {
            filter: filter.to_owned(),
            ..PipelineOpts::default()
        },
    )?;
    Ok(match data {
        Value::Array(items) => items,
        _ => Vec::new(),
    })
}

/// How many items a [`paginate_known_collection`] call will look at: the
/// window's end plus one, because seeing an item past the window is what says
/// there is a next page. A caller that gathers the collection from a paged
/// service can stop reading once it holds this many.
pub(crate) fn known_collection_need(
    continue_token: Option<&str>,
    limit: i64,
    max_limit: i64,
) -> cli_engine::Result<usize> {
    let fields = parse(continue_token, &["limit", "offset"])?;
    let offset = fields.typed::<usize>("offset")?.unwrap_or(0);
    let size = saturating_usize(limit_from(&fields, limit, max_limit)?).max(1);
    Ok(offset.saturating_add(size).saturating_add(1))
}

/// Drops `total` and `remaining` from a continuation whose collection was only
/// read as far as the window needed: the length of a prefix is not a count.
pub(crate) fn forget_totals(mut continuation: CursorContinuation) -> CursorContinuation {
    continuation.total = None;
    continuation.remaining = None;
    continuation
}

/// Slices an already-fully-known collection using a `limit:<n>,offset:<n>`
/// continuation token.
pub(crate) fn paginate_known_collection(
    items: Vec<Value>,
    limit: i64,
    max_limit: i64,
    continue_token: Option<&str>,
) -> cli_engine::Result<(Vec<Value>, CursorContinuation)> {
    let fields = parse(continue_token, &["limit", "offset"])?;
    let offset = fields.typed::<usize>("offset")?.unwrap_or(0);
    let limit = saturating_usize(limit_from(&fields, limit, max_limit)?).max(1);
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
    Ok((page, continuation))
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

    const KEYS: &[&str] = &["page", "limit", "offset"];

    #[test]
    fn parse_reads_key_value_pairs() {
        let fields = parse(Some("page:2,limit:50"), KEYS).expect("valid");
        assert_eq!(fields.text("page"), Some("2"));
        assert_eq!(fields.text("limit"), Some("50"));
    }

    #[test]
    fn parse_of_none_or_blank_is_no_fields() {
        for token in [None, Some(""), Some("   ")] {
            let fields = parse(token, KEYS).expect("start from the beginning");
            assert_eq!(fields.text("page"), None);
        }
    }

    #[test]
    fn parse_rejects_a_pair_with_no_colon() {
        for bad in ["page:2,garbage,limit:50", "limit:5,offs", "garbage"] {
            let err = parse(Some(bad), KEYS).expect_err(bad);
            assert!(
                err.to_string().contains("invalid --continue value"),
                "{bad}: {err}"
            );
        }
    }

    #[test]
    fn parse_rejects_a_key_this_command_never_issues() {
        // e.g. a `dns list` token (`page`) pasted into a client-side list.
        let err = parse(Some("page:2"), &["limit", "offset"]).expect_err("foreign key");
        assert!(err.to_string().contains("`page`"), "{err}");
    }

    #[test]
    fn invalid_token_errors_carry_an_actionable_fix_and_a_bounded_echo() {
        let long = "x".repeat(500);
        let err = parse(Some(&long), KEYS).expect_err("garbage");
        let text = format!("{err:?}");
        assert!(text.contains("previous page's next action"), "{text}");
        assert!(
            err.to_string().len() < 300,
            "echo of the bad value is bounded"
        );
    }

    #[test]
    fn typed_fields_are_absent_ok_or_an_error() {
        let fields = parse(Some("page:3,limit:abc"), KEYS).expect("well-formed");
        assert_eq!(fields.typed::<u32>("page").expect("valid"), Some(3));
        assert_eq!(fields.typed::<u32>("offset").expect("absent"), None);
        let err = fields.typed::<u32>("limit").expect_err("not a number");
        assert!(err.to_string().contains("`limit`"), "{err}");
        // A negative or fractional page is not a u32 either.
        let fields = parse(Some("page:-1"), KEYS).expect("well-formed");
        assert!(fields.typed::<u32>("page").is_err());
    }

    #[test]
    fn build_and_parse_round_trip() {
        let token = build(&[("page", "3"), ("limit", "20")]);
        let fields = parse(Some(&token), KEYS).expect("valid");
        assert_eq!(fields.typed::<u32>("page").expect("valid"), Some(3));
        assert_eq!(fields.typed::<u32>("limit").expect("valid"), Some(20));
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
        let (page, page_size) =
            resolve_page_request(None::<u32>, None, 1u32, 25, 100).expect("valid token");
        assert_eq!(page, 1);
        assert_eq!(page_size, 25);
    }

    #[test]
    fn resolve_page_request_inherits_both_page_and_size_from_the_continue_token() {
        // The size that produced page 2 (10) must carry forward even though
        // this call's own --limit defaulted to something else (25) — page 2
        // of size-10 pages and page 2 of size-25 pages cover different items.
        let token = build(&[("limit", "10"), ("page", "2")]);
        let (page, page_size) =
            resolve_page_request(None::<u32>, Some(&token), 1u32, 25, 100).expect("valid token");
        assert_eq!(page, 2);
        assert_eq!(page_size, 10);
    }

    #[test]
    fn resolve_page_request_an_explicit_page_is_used_without_a_token_and_takes_the_requests_limit()
    {
        // No continue token, so there's no prior size to inherit — the
        // request's own limit applies.
        let (page, page_size) =
            resolve_page_request(Some(5u32), None, 1u32, 25, 100).expect("valid token");
        assert_eq!(page, 5);
        assert_eq!(page_size, 25);
    }

    #[test]
    fn resolve_page_request_a_continue_token_wins_over_an_explicit_page() {
        // The engine's next-page action replays `--page 5` next to the new
        // `--continue`; the token's page has to win or the user never advances.
        let token = build(&[("limit", "10"), ("page", "6")]);
        let (page, page_size) =
            resolve_page_request(Some(5u32), Some(&token), 1u32, 25, 100).expect("valid token");
        assert_eq!(page, 6);
        assert_eq!(page_size, 10);
    }

    fn items(n: usize) -> Vec<Value> {
        (0..n).map(|i| json!({ "id": i })).collect()
    }

    #[test]
    fn paginate_known_collection_returns_the_first_page_with_no_token() {
        let (page, cont) = paginate_known_collection(items(5), 2, 100, None).expect("valid token");
        assert_eq!(page, vec![json!({"id": 0}), json!({"id": 1})]);
        assert!(cont.continue_from.is_some());
        assert_eq!(cont.total, Some(5));
        assert_eq!(cont.remaining, Some(3));
    }

    #[test]
    fn paginate_known_collection_resumes_from_the_continue_token() {
        let (_, cont) = paginate_known_collection(items(5), 2, 100, None).expect("valid token");
        let token = cont.continue_from.expect("has_more implies a token");
        let (page, cont2) =
            paginate_known_collection(items(5), 2, 100, Some(&token)).expect("valid token");
        assert_eq!(page, vec![json!({"id": 2}), json!({"id": 3})]);
        assert!(cont2.continue_from.is_some());
        assert_eq!(cont2.remaining, Some(1));
    }

    #[test]
    fn paginate_known_collection_continue_token_embeds_and_inherits_the_limit() {
        let (_, cont) = paginate_known_collection(items(5), 2, 100, None).expect("valid token");
        let token = cont.continue_from.expect("has_more implies a token");
        assert_eq!(token, "limit:2,offset:2");

        // A caller that resumes with --continue alone, and whose --limit
        // defaulted to something else entirely (25) this call, must still
        // get the size (2) that produced the token, not the new default.
        let (page, cont2) =
            paginate_known_collection(items(5), 25, 100, Some(&token)).expect("valid token");
        assert_eq!(page, vec![json!({"id": 2}), json!({"id": 3})]);
        assert_eq!(cont2.remaining, Some(1));
    }

    #[test]
    fn paginate_known_collection_an_explicit_offset_token_has_no_limit_to_inherit() {
        // resolve_continue_token synthesizes a bare `offset:<n>` (no
        // `limit`) for an explicit --offset — the request's own limit
        // applies, same as a fresh, tokenless call.
        let token = resolve_continue_token(Some(2), None).expect("explicit offset");
        let (page, _) =
            paginate_known_collection(items(5), 3, 100, Some(&token)).expect("valid token");
        assert_eq!(
            page,
            vec![json!({"id": 2}), json!({"id": 3}), json!({"id": 4})]
        );
    }

    #[test]
    fn paginate_known_collection_reaches_the_end_with_no_further_token() {
        let (page, cont) =
            paginate_known_collection(items(5), 2, 100, Some("offset:4")).expect("valid token");
        assert_eq!(page, vec![json!({"id": 4})]);
        assert!(cont.continue_from.is_none());
        assert_eq!(cont.remaining, Some(0));
    }

    #[test]
    fn paginate_known_collection_past_the_end_returns_empty() {
        let (page, cont) =
            paginate_known_collection(items(3), 2, 100, Some("offset:10")).expect("valid token");
        assert!(page.is_empty());
        assert!(cont.continue_from.is_none());
        assert_eq!(cont.remaining, Some(0));
    }

    #[test]
    fn paginate_known_collection_of_an_empty_collection() {
        let (page, cont) =
            paginate_known_collection(Vec::new(), 10, 100, None).expect("valid token");
        assert!(page.is_empty());
        assert!(cont.continue_from.is_none());
        assert_eq!(cont.total, Some(0));
    }

    #[test]
    fn known_collection_need_is_the_window_end_plus_one() {
        let need = |token: Option<&str>, limit| known_collection_need(token, limit, 100);
        assert_eq!(need(None, 10).expect("valid"), 11);
        assert_eq!(need(Some("limit:5,offset:20"), 10).expect("valid"), 26);
        assert_eq!(need(Some("offset:3"), 7).expect("valid"), 11);
        assert_eq!(
            need(Some("offset:18446744073709551615"), 10).expect("valid"),
            usize::MAX
        );
    }

    #[test]
    fn known_collection_need_rejects_a_token_that_slicing_would_reject() {
        for bad in [
            "limit:-1,offset:20",
            "limit:0",
            "limit:101",
            "offset:-3",
            "nonsense",
        ] {
            assert!(known_collection_need(Some(bad), 10, 100).is_err(), "{bad}");
        }
    }

    #[test]
    fn forget_totals_clears_only_the_counts() {
        let (_, cont) = paginate_known_collection(items(5), 2, 100, None).expect("valid token");
        assert_eq!((cont.total, cont.remaining), (Some(5), Some(3)));
        let cont = forget_totals(cont);
        assert_eq!((cont.total, cont.remaining), (None, None));
        assert_eq!(cont.continue_from.as_deref(), Some("limit:2,offset:2"));
    }

    #[test]
    fn filter_known_collection_keeps_only_matching_items_in_order() {
        let all = vec![
            json!({ "name": "a.one" }),
            json!({ "name": "b.two" }),
            json!({ "name": "a.three" }),
        ];
        let kept =
            filter_known_collection(all.clone(), "starts_with(name, 'a.')").expect("filters");
        assert_eq!(
            kept,
            vec![json!({ "name": "a.one" }), json!({ "name": "a.three" })]
        );
        assert_eq!(
            filter_known_collection(all.clone(), "").expect("no filter"),
            all
        );
    }

    #[test]
    fn filter_known_collection_propagates_an_invalid_expression() {
        let err = filter_known_collection(vec![json!({ "name": "x" })], "starts_with(((")
            .expect_err("an invalid filter must surface, not be swallowed");
        assert!(!err.to_string().is_empty());
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
            let fields = parse(Some(&token), &["token", "direction"]).expect("valid");
            assert_eq!(fields.text("token"), Some(opaque), "{token}");
            assert_eq!(fields.text("direction"), Some("forward"));
        }
    }

    #[test]
    fn a_comma_in_a_value_does_not_split_the_token() {
        assert_eq!(build(&[("token", "abc,def")]), "token:abc%2Cdef");
    }

    #[test]
    fn paginate_known_collection_rejects_an_out_of_range_token_limit() {
        // Negative would otherwise convert to usize::MAX and return everything;
        // zero and oversized values are not sizes this command ever issues.
        for bad in [
            "limit:-1,offset:0",
            "limit:0,offset:0",
            "limit:101,offset:0",
        ] {
            let err = paginate_known_collection(items(300), 7, 100, Some(bad)).expect_err(bad);
            assert!(err.to_string().contains("`limit`"), "{bad}: {err}");
        }
        let (page, _) = paginate_known_collection(items(300), 7, 100, Some("limit:100,offset:0"))
            .expect("in range");
        assert_eq!(
            page.len(),
            100,
            "an in-range token limit is still inherited"
        );
    }

    #[test]
    fn paginate_known_collection_rejects_a_garbled_or_foreign_token() {
        for bad in [
            "limit:5,offs",
            "garbage",
            "offset:abc",
            "offset:-3",
            "page:2",
        ] {
            assert!(
                paginate_known_collection(items(10), 5, 100, Some(bad)).is_err(),
                "{bad}"
            );
        }
        // Blank is "start from the beginning", not an error.
        for blank in [None, Some(""), Some("  ")] {
            let (page, _) = paginate_known_collection(items(10), 5, 100, blank).expect("blank");
            assert_eq!(page.len(), 5);
        }
    }

    #[test]
    fn resolve_page_request_rejects_a_zero_page_token_for_every_page_type() {
        // dns list reads the page as u64 and clamps it, email list as u32; either
        // would otherwise act on page one while the token claims page zero.
        let err = resolve_page_request(None::<u64>, Some("limit:25,page:0"), 1u64, 25, 100)
            .expect_err("zero page");
        assert!(err.to_string().contains("at least 1"), "{err}");
        assert!(resolve_page_request(None::<u32>, Some("limit:25,page:0"), 1u32, 25, 100).is_err());
        // A valid token page passes (an explicit --page is validated by clap).
        assert!(resolve_page_request(None::<u64>, Some("limit:25,page:1"), 1u64, 25, 100).is_ok());
    }

    #[test]
    fn resolve_page_request_rejects_an_out_of_range_or_garbled_token() {
        for bad in [
            "limit:-1,page:2",
            "limit:0,page:2",
            "limit:101,page:2",
            "limit:25,page:abc",
            "limit:25,page:0",
            "limit:25,pag",
            "offset:5",
        ] {
            assert!(
                resolve_page_request(None::<u32>, Some(bad), 1u32, 25, 100).is_err(),
                "{bad}"
            );
        }
        let (page, size) =
            resolve_page_request(None::<u32>, Some("limit:100,page:2"), 1u32, 25, 100)
                .expect("in range");
        assert_eq!((page, size), (2, 100));
    }
}
