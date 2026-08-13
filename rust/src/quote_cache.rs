//! Local cache of registration quotes, bridging `domain quote` → `domain purchase`.
//!
//! v3's `POST /registrations` needs more than the opaque `quoteToken`: it must
//! re-state the quote's `domain`, `period`, and `consent.agreementTypes` (which the
//! server cross-checks against the quote the token was minted for). The token is
//! opaque ("do not parse") and there is **no GET-quote endpoint**, so those values
//! can't be recovered from the token or re-fetched from the server. `domain quote`
//! therefore stashes the quote here and `domain purchase --quote-token` reads it
//! back — reuniting the token with the data the register body must echo, and
//! letting `purchase` charge the exact price that was reviewed.
//!
//! Entries are single-use: [`get`] reads a quote without consuming it (so the
//! `--agree` gate can show its terms before the user confirms), and [`remove`]
//! deletes it only after the registration succeeds. They also expire with the
//! token (~10 min), and expired entries are pruned whenever the cache is read
//! ([`get`]) or written ([`save`]) — so an abandoned quote's data (including a
//! serialized profile that may contain contact PII) doesn't linger past its
//! expiry once any later quote/purchase runs, and the file stays small. It
//! lives beside `contacts.toml`/`environments.toml`
//! (`dirs::config_dir()/gddy/quotes.json`); a quote token is a short-lived,
//! single-use capability, not a long-lived secret.
//!
//! (The domains team is being asked to add a `GET /registration-quotes/{token}`
//! route; if that lands, `purchase` could resolve the quote server-side and this
//! local cache could be retired.)

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// One cached quote: everything `domain purchase` needs to build the register
/// request that the `quoteToken` was minted for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CachedQuote {
    pub domain: String,
    pub period: u64,
    /// The `agreementType` values from the quote's `requiredAgreements` — echoed
    /// into `consent.agreementTypes`, which the server verifies against the quote.
    pub agreement_types: Vec<String>,
    /// Human-readable agreement titles (+ optional URLs), for the `--agree` review
    /// prompt at purchase time. Parallel to `agreement_types`.
    #[serde(default)]
    pub agreement_titles: Vec<String>,
    /// The locked price, pre-formatted for the receipt (e.g. "11.99").
    #[serde(default)]
    pub price: Option<String>,
    #[serde(default)]
    pub currency: Option<String>,
    /// RFC 3339 expiry of the quote token (from the quote's `expiresAt`); `None`
    /// if the API returned no expiry (then the entry never expires locally).
    #[serde(default)]
    pub expires_at: Option<String>,
    /// The exact `InlineRegistrationProfile` the quote was taken with, serialized.
    /// `register` must re-send it verbatim or the server rejects the mismatch
    /// (the token binds a hash of the domain/price/profile). `None` when the quote
    /// carried no profile.
    #[serde(default)]
    pub profile: Option<serde_json::Value>,
    /// Idempotency key for the `register` call, minted once at quote time and
    /// reused on every purchase attempt for this token. Reusing it means a retry
    /// after a lost/timed-out response is recognized server-side as the same
    /// request rather than risking a second charge. `None` only for entries
    /// written by an older CLI, where `purchase` falls back to generating one.
    #[serde(default)]
    pub idempotency_key: Option<String>,
    /// The quote's `fees` array (e.g. a premium domain's one-time acquisition
    /// charge), serialized verbatim. `register` must echo these back into
    /// `consent.acknowledgedFees` exactly (same types/amounts/currencies) or the
    /// server rejects the execute with `422 quote_mismatch`. `None` when the
    /// quote carried no fees (the common, non-premium case) or for quotes cached
    /// by an older CLI that predates premium-domain support.
    #[serde(default)]
    pub fees: Option<serde_json::Value>,
}

/// The result of looking a token up in the cache.
pub enum Lookup {
    /// The quote was found. Not consumed here — [`get`] is read-only; the entry
    /// is removed (via [`remove`]) only once the registration succeeds.
    Found(Box<CachedQuote>),
    /// The token was present but its quote had expired.
    Expired,
    /// No quote for this token on this machine.
    Missing,
    /// No config directory could be resolved, so the cache can't be located at
    /// all (distinct from a genuinely absent quote — e.g. a stripped container).
    NoConfigDir,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct QuoteFile {
    #[serde(default)]
    quotes: BTreeMap<String, CachedQuote>,
}

/// Path to the local quotes cache, if a config dir can be resolved. Mirrors
/// [`crate::contacts::contacts_path`] (same `gddy/` config directory).
pub fn quotes_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("gddy").join("quotes.json"))
}

/// Whether a cached quote has expired at `now`. An *absent* `expires_at` is
/// treated as not-expired (the API always returns one in practice; a quote with
/// no expiry never expires locally). An *unparseable* one is treated as expired:
/// a malformed timestamp only arises from corruption, a manual edit, or an older
/// format, so the entry is already suspect — expiring it avoids keeping its
/// serialized profile (potential PII) on disk indefinitely.
fn is_expired(quote: &CachedQuote, now: chrono::DateTime<chrono::Utc>) -> bool {
    match quote.expires_at.as_deref() {
        Some(ts) => chrono::DateTime::parse_from_rfc3339(ts)
            .map(|exp| exp.with_timezone(&chrono::Utc) <= now)
            .unwrap_or(true),
        None => false,
    }
}

/// Load the cache file. A missing or unparseable file yields an empty cache — it
/// is only a best-effort bridge, so a corrupt file must never brick `quote`/`purchase`.
fn load(path: &Path) -> QuoteFile {
    match std::fs::read_to_string(path) {
        Ok(contents) => serde_json::from_str(&contents).unwrap_or_default(),
        Err(_) => QuoteFile::default(),
    }
}

fn write(path: &Path, file: &QuoteFile) -> cli_engine::Result<()> {
    let json = serde_json::to_string_pretty(file).map_err(|e| {
        cli_engine::CliCoreError::message(format!("failed to serialize quotes: {e}"))
    })?;
    cli_engine::fs::write_string_atomic(path, &json)
}

/// Persist a quote under its token, first pruning any expired entries so the file
/// stays small. Best-effort: callers surface a warning on failure (the quote is
/// still usable interactively; only `purchase --quote-token` depends on it).
pub fn save(token: &str, quote: CachedQuote) -> cli_engine::Result<()> {
    let path = quotes_path().ok_or_else(|| {
        cli_engine::CliCoreError::message("no config directory for the quote cache")
    })?;
    save_at(&path, chrono::Utc::now(), token, quote)
}

fn save_at(
    path: &Path,
    now: chrono::DateTime<chrono::Utc>,
    token: &str,
    quote: CachedQuote,
) -> cli_engine::Result<()> {
    let mut file = load(path);
    file.quotes.retain(|_, q| !is_expired(q, now));
    file.quotes.insert(token.to_owned(), quote);
    write(path, &file)
}

/// Look up the quote for `token` **without** consuming it. Distinguishes an
/// expired quote from one that was never cached here so `purchase` can give a
/// precise error. Read-only on purpose: `purchase` reads the quote to show the
/// agreements (the `--agree` gate) and only [`remove`]s it once the registration
/// actually succeeds, so an aborted/gated attempt leaves the quote reusable.
pub fn get(token: &str) -> Lookup {
    let Some(path) = quotes_path() else {
        return Lookup::NoConfigDir;
    };
    let now = chrono::Utc::now();
    // Best-effort: drop any expired entries (whose serialized profile may hold
    // contact PII) so an abandoned quote doesn't linger on disk past its expiry.
    prune_expired(&path, now);
    get_at(&path, now, token)
}

/// Remove every expired entry from the cache file, rewriting it only if any were
/// dropped. Best-effort — a write failure just leaves stale entries (which are
/// still treated as expired on read and pruned on the next successful write).
fn prune_expired(path: &Path, now: chrono::DateTime<chrono::Utc>) {
    let mut file = load(path);
    let before = file.quotes.len();
    file.quotes.retain(|_, q| !is_expired(q, now));
    if file.quotes.len() != before
        && let Err(e) = write(path, &file)
    {
        tracing::warn!(error = %e, "could not prune expired quotes from the cache");
    }
}

fn get_at(path: &Path, now: chrono::DateTime<chrono::Utc>, token: &str) -> Lookup {
    let file = load(path);
    match file.quotes.get(token) {
        None => Lookup::Missing,
        Some(q) if is_expired(q, now) => Lookup::Expired,
        Some(q) => Lookup::Found(Box::new(q.clone())),
    }
}

/// Consume a quote once its registration has succeeded (single-use). Best-effort:
/// a write failure only risks a stale entry (which expires shortly anyway), so it
/// warns rather than failing the completed purchase. A no-op if the token is absent.
pub fn remove(token: &str) {
    let Some(path) = quotes_path() else {
        return;
    };
    let mut file = load(&path);
    if file.quotes.remove(token).is_some()
        && let Err(e) = write(&path, &file)
    {
        tracing::warn!(error = %e, "could not update the quote cache after a purchase");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(y: i32, mo: u32, d: u32, h: u32) -> chrono::DateTime<chrono::Utc> {
        chrono::Utc
            .with_ymd_and_hms(y, mo, d, h, 0, 0)
            .single()
            .expect("valid instant")
    }

    fn quote(domain: &str, expires: Option<&str>) -> CachedQuote {
        CachedQuote {
            domain: domain.to_owned(),
            period: 1,
            agreement_types: vec!["DNRA".to_owned()],
            agreement_titles: vec!["Registration Agreement".to_owned()],
            price: Some("11.99".to_owned()),
            currency: Some("USD".to_owned()),
            expires_at: expires.map(str::to_owned),
            profile: None,
            idempotency_key: Some("idem-test".to_owned()),
            fees: None,
        }
    }

    fn remove_at(path: &Path, token: &str) {
        let mut file = load(path);
        if file.quotes.remove(token).is_some() {
            write(path, &file).expect("write");
        }
    }

    #[test]
    fn get_is_read_only_then_remove_consumes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("quotes.json");
        let now = at(2026, 7, 1, 12);

        save_at(
            &path,
            now,
            "tok-1",
            quote("example.com", Some("2026-07-01T12:09:00Z")),
        )
        .expect("save");

        // get returns the quote and, crucially, does NOT consume it (the --agree
        // gate reads it before the user confirms), so two reads both succeed.
        let mut found = 0;
        for _ in 0..2 {
            if let Lookup::Found(q) = get_at(&path, now, "tok-1") {
                assert_eq!(q.domain, "example.com");
                assert_eq!(q.agreement_types, vec!["DNRA".to_owned()]);
                found += 1;
            }
        }
        assert_eq!(
            found, 2,
            "get must return Found without consuming the quote"
        );

        // remove consumes it (single-use, on successful purchase).
        remove_at(&path, "tok-1");
        assert!(matches!(get_at(&path, now, "tok-1"), Lookup::Missing));
    }

    #[test]
    fn expired_token_reports_expired_not_found() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("quotes.json");
        // Saved at noon, expires 12:09; read at 13:00 → expired.
        save_at(
            &path,
            at(2026, 7, 1, 12),
            "tok-1",
            quote("example.com", Some("2026-07-01T12:09:00Z")),
        )
        .expect("save");
        assert!(matches!(
            get_at(&path, at(2026, 7, 1, 13), "tok-1"),
            Lookup::Expired
        ));
    }

    #[test]
    fn unparseable_expiry_is_treated_as_expired() {
        // A malformed timestamp (corruption/manual edit/old format) must not keep
        // an entry — and its potential PII — alive forever.
        let q = quote("x.com", Some("not-a-timestamp"));
        assert!(is_expired(&q, at(2026, 7, 1, 12)));
        // Absent expiry still means never-expires-locally.
        let no_expiry = quote("y.com", None);
        assert!(!is_expired(&no_expiry, at(2026, 7, 1, 12)));
    }

    #[test]
    fn unknown_token_is_missing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("quotes.json");
        assert!(matches!(
            get_at(&path, at(2026, 7, 1, 12), "nope"),
            Lookup::Missing
        ));
    }

    #[test]
    fn save_prunes_expired_entries() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("quotes.json");
        let early = at(2026, 7, 1, 12);
        save_at(
            &path,
            early,
            "old",
            quote("old.com", Some("2026-07-01T12:05:00Z")),
        )
        .expect("save");
        // A later save (past the first's expiry) should drop the stale entry.
        let later = at(2026, 7, 1, 13);
        save_at(
            &path,
            later,
            "new",
            quote("new.com", Some("2026-07-01T13:09:00Z")),
        )
        .expect("save");

        assert!(matches!(get_at(&path, later, "old"), Lookup::Missing));
        assert!(matches!(get_at(&path, later, "new"), Lookup::Found(_)));
    }

    #[test]
    fn prune_expired_removes_stale_entries_from_disk() {
        // Bounds PII-at-rest: an expired quote is physically removed on read,
        // not just masked as expired, so an abandoned quote can't linger.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("quotes.json");
        let saved = at(2026, 7, 1, 12);
        save_at(
            &path,
            saved,
            "old",
            quote("old.com", Some("2026-07-01T12:30:00Z")),
        )
        .expect("save");
        save_at(
            &path,
            saved,
            "live",
            quote("live.com", Some("2026-07-01T14:00:00Z")),
        )
        .expect("save");

        // At 13:00 "old" (expires 12:30) is stale; "live" (expires 14:00) isn't.
        prune_expired(&path, at(2026, 7, 1, 13));

        let remaining = load(&path).quotes;
        assert!(
            !remaining.contains_key("old"),
            "expired entry must be pruned from disk"
        );
        assert!(remaining.contains_key("live"), "live entry must remain");
    }
}
