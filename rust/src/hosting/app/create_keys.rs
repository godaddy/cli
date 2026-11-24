//! Request keys of unfinished `hosting app create` runs.
//!
//! A create can reach the service and still fail on the way back (a timeout, a
//! dropped connection, a server error). Run again without the first request's
//! key, it would create a second app in a paid slot. So the key is saved before
//! the request and reused while the same environment, app type, hosting product
//! and name are created again; the service then returns the first request's
//! operation. The entry is removed once the service accepts the create.
//!
//! The service keeps a key for 24 hours, so entries older than that are dropped
//! whenever the file is read. The name is matched exactly as typed, because the
//! service compares the name exactly as sent: `"a "` and `"a"` are different
//! requests to it. The file lives beside the other `gddy` files
//! (`dirs::config_dir()/gddy/hosting-create-keys.json`).
//!
//! Saving is best effort: a create without a saved key still works, it just
//! cannot be made safe to run again, and the caller's error text says so.

use std::path::{Path, PathBuf};

use chrono::{DateTime, TimeDelta, Utc};
use serde::{Deserialize, Serialize};

use crate::hosting::common::{AppHostingProduct, HostingAppType};

const FILE_NAME: &str = "hosting-create-keys.json";

/// How long the service replays a key, counted from the first attempt.
const KEY_TTL: TimeDelta = TimeDelta::hours(24);

/// A run of the same command is promised to resend the key only while at least
/// this much of its time is left, so that there is time to read the error and
/// run the command before the key expires.
const RERUN_MARGIN: TimeDelta = TimeDelta::minutes(10);

#[derive(Debug, Default, Serialize, Deserialize)]
struct KeyFile {
    #[serde(default)]
    entries: Vec<Entry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Entry {
    base_url: String,
    app_type: String,
    /// The `--hosting-product` asked for, if any. A create that names one is a
    /// different request from one that lets the service choose.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    hosting_product: Option<String>,
    name: String,
    key: String,
    created_at: DateTime<Utc>,
}

impl Entry {
    fn is_for(&self, base_url: &str, create: Create<'_>) -> bool {
        self.base_url == base_url
            && self.app_type == create.app_type.as_str()
            && self.hosting_product.as_deref()
                == create.hosting_product.map(AppHostingProduct::as_str)
            && self.name == create.name
    }
}

/// What a create asks for: the part of the request, besides the environment,
/// that a saved key belongs to.
#[derive(Debug, Clone, Copy)]
pub(super) struct Create<'a> {
    pub(super) app_type: HostingAppType,
    pub(super) hosting_product: Option<AppHostingProduct>,
    pub(super) name: &'a str,
}

/// The key to send with a create.
#[derive(Debug)]
pub(super) struct Claim {
    pub(super) key: String,
    /// When a run of the same command stops resending the key: the first
    /// attempt plus the time the service replays it. `None` when the key could
    /// not be saved, so running the command again would not reuse it.
    pub(super) expires_at: Option<DateTime<Utc>>,
}

impl Claim {
    /// The time before which running the same command again resends this key,
    /// or `None` when it would not: the key is not saved, or too little of its
    /// time is left to promise it.
    pub(super) fn rerun_deadline(&self, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
        self.expires_at.filter(|&at| now + RERUN_MARGIN < at)
    }
}

/// Path to the key file, if a config directory can be resolved.
pub(super) fn path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("gddy").join(FILE_NAME))
}

/// Returns the saved key for this create, or a new one, saved before it is
/// returned so that a create interrupted at any point can be run again.
pub(super) fn claim(
    path: Option<&Path>,
    base_url: &str,
    create: Create<'_>,
    now: DateTime<Utc>,
) -> Claim {
    let Some(path) = path else {
        tracing::warn!("no config directory: the hosting create key is not saved");
        return Claim {
            key: new_key(),
            expires_at: None,
        };
    };
    let mut file = load(path, now);
    if let Some(entry) = file.entries.iter().find(|e| e.is_for(base_url, create)) {
        return Claim {
            key: entry.key.clone(),
            expires_at: Some(entry.created_at + KEY_TTL),
        };
    }
    let key = new_key();
    file.entries.push(Entry {
        base_url: base_url.to_owned(),
        app_type: create.app_type.as_str().to_owned(),
        hosting_product: create
            .hosting_product
            .map(|product| product.as_str().to_owned()),
        name: create.name.to_owned(),
        key: key.clone(),
        created_at: now,
    });
    let expires_at = match write(path, &file) {
        Ok(()) => Some(now + KEY_TTL),
        Err(error) => {
            tracing::warn!(%error, "could not save the hosting create key");
            None
        }
    };
    Claim { key, expires_at }
}

/// Forgets the key for this create, once the service has accepted it or said
/// that it no longer fits. Returns false when the key is still saved because
/// the file could not be written, so running the command again would resend it.
pub(super) fn release(
    path: Option<&Path>,
    base_url: &str,
    create: Create<'_>,
    now: DateTime<Utc>,
) -> bool {
    let Some(path) = path else {
        return true;
    };
    let mut file = load(path, now);
    let before = file.entries.len();
    file.entries.retain(|e| !e.is_for(base_url, create));
    if file.entries.len() == before {
        return true;
    }
    match write(path, &file) {
        Ok(()) => true,
        Err(error) => {
            tracing::warn!(%error, "could not remove the hosting create key");
            false
        }
    }
}

fn new_key() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Loads the file without expired entries. A missing or unreadable file reads
/// as empty: this file only makes a re-run safe, it must never block a create.
fn load(path: &Path, now: DateTime<Utc>) -> KeyFile {
    let mut file: KeyFile = std::fs::read_to_string(path)
        .ok()
        .and_then(|contents| serde_json::from_str(&contents).ok())
        .unwrap_or_default();
    file.entries.retain(|e| now - e.created_at < KEY_TTL);
    file
}

fn write(path: &Path, file: &KeyFile) -> cli_engine::Result<()> {
    let json = serde_json::to_string_pretty(file).map_err(|e| {
        cli_engine::CliCoreError::message(format!("failed to serialize hosting create keys: {e}"))
    })?;
    cli_engine::fs::write_string_atomic(path, &json)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROD: &str = "https://api.godaddy.com";

    /// A WordPress create of `name`.
    const fn site(name: &str) -> Create<'_> {
        Create {
            app_type: HostingAppType::Mhwp,
            hosting_product: None,
            name,
        }
    }

    fn now() -> DateTime<Utc> {
        "2026-11-13T12:00:00Z".parse().expect("valid time")
    }

    fn entries(path: &Path) -> Vec<Entry> {
        load(path, now()).entries
    }

    #[test]
    fn the_same_create_reuses_its_saved_key() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(FILE_NAME);

        let first = claim(Some(&path), PROD, site("my-site"), now());
        let again = claim(Some(&path), PROD, site("my-site"), now());

        assert_eq!(first.expires_at, Some(now() + KEY_TTL));
        assert_eq!(again.expires_at, first.expires_at);
        assert_eq!(first.key, again.key);
        assert!(uuid::Uuid::parse_str(&first.key).is_ok(), "{}", first.key);
        assert_eq!(entries(&path).len(), 1);
    }

    #[test]
    fn another_environment_type_product_or_name_gets_its_own_key() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(FILE_NAME);
        let base = claim(Some(&path), PROD, site("my-site"), now()).key;
        let nodejs = |hosting_product| Create {
            app_type: HostingAppType::Nodejs,
            hosting_product,
            name: "my-site",
        };

        let mut keys = vec![base];
        for (url, create) in [
            ("https://api.ote-godaddy.com", site("my-site")),
            (PROD, site("other-site")),
            // The service compares the name as sent, so a space makes it another request.
            (PROD, site("my-site ")),
            (PROD, nodejs(None)),
            // Naming a product asks for something else than letting the service choose.
            (PROD, nodejs(Some(AppHostingProduct::Airo))),
            (PROD, nodejs(Some(AppHostingProduct::WebHosting))),
        ] {
            let key = claim(Some(&path), url, create, now()).key;
            assert!(!keys.contains(&key), "{url} {create:?}");
            keys.push(key);
        }
        assert_eq!(entries(&path).len(), keys.len());

        // Each is still found again, and released on its own.
        let airo = nodejs(Some(AppHostingProduct::Airo));
        assert!(keys.contains(&claim(Some(&path), PROD, airo, now()).key));
        assert!(release(Some(&path), PROD, airo, now()));
        assert_eq!(entries(&path).len(), keys.len() - 1);
    }

    #[test]
    fn a_key_saved_before_hosting_products_still_matches_a_plain_create() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(FILE_NAME);
        let saved = serde_json::json!({ "entries": [{
            "baseUrl": PROD, "appType": "MHWP", "name": "my-site",
            "key": "5d3c7f0e-2b8a-4f61-9a0d-7c1e3b2a4f50", "createdAt": now(),
        }] });
        std::fs::write(&path, saved.to_string()).expect("write key file");

        let claimed = claim(Some(&path), PROD, site("my-site"), now());

        assert_eq!(claimed.key, "5d3c7f0e-2b8a-4f61-9a0d-7c1e3b2a4f50");
    }

    #[test]
    fn a_key_older_than_a_day_is_replaced_and_dropped() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(FILE_NAME);
        let old = claim(Some(&path), PROD, site("my-site"), now()).key;

        let later = now() + KEY_TTL;
        let new = claim(Some(&path), PROD, site("my-site"), later).key;

        assert_ne!(new, old);
        let left = load(&path, later).entries;
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].key, new);
    }

    #[test]
    fn a_rerun_does_not_extend_the_first_attempt() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(FILE_NAME);
        claim(Some(&path), PROD, site("my-site"), now());
        let later = now() + TimeDelta::hours(1);
        let again = claim(Some(&path), PROD, site("my-site"), later);

        assert_eq!(entries(&path)[0].created_at, now());
        assert_eq!(again.expires_at, Some(now() + KEY_TTL));
    }

    #[test]
    fn a_rerun_is_promised_only_while_enough_of_the_key_time_is_left() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(FILE_NAME);
        let claimed = claim(Some(&path), PROD, site("my-site"), now());
        let expires = now() + KEY_TTL;

        assert_eq!(claimed.rerun_deadline(now()), Some(expires));
        let last_safe = expires - RERUN_MARGIN - TimeDelta::seconds(1);
        assert_eq!(claimed.rerun_deadline(last_safe), Some(expires));
        assert_eq!(claimed.rerun_deadline(expires - RERUN_MARGIN), None);
        assert_eq!(claimed.rerun_deadline(expires), None);

        let unsaved = claim(None, PROD, site("my-site"), now());
        assert_eq!(unsaved.rerun_deadline(now()), None);
    }

    #[test]
    fn a_run_before_the_deadline_resends_the_key() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(FILE_NAME);
        let first = claim(Some(&path), PROD, site("my-site"), now());
        let deadline = first.rerun_deadline(now()).expect("deadline");

        let just_before = deadline - TimeDelta::seconds(1);
        let again = claim(Some(&path), PROD, site("my-site"), just_before);
        assert_eq!(again.key, first.key);
        let at_deadline = claim(Some(&path), PROD, site("my-site"), deadline);
        assert_ne!(at_deadline.key, first.key);
    }

    #[test]
    fn release_removes_only_its_own_entry() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(FILE_NAME);
        claim(Some(&path), PROD, site("my-site"), now());
        let other = claim(Some(&path), PROD, site("other-site"), now()).key;

        assert!(release(Some(&path), PROD, site("my-site"), now()));

        let left = entries(&path);
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].key, other);
        let fresh = claim(Some(&path), PROD, site("my-site"), now());
        assert!(fresh.expires_at.is_some());
    }

    #[test]
    fn release_of_a_key_that_is_not_saved_succeeds() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(FILE_NAME);

        assert!(release(Some(&path), PROD, site("my-site"), now()));
        assert!(release(None, PROD, site("my-site"), now()));
        assert!(!path.exists(), "nothing to remove, so nothing is written");
    }

    #[cfg(unix)]
    #[test]
    fn release_reports_a_key_it_could_not_remove() {
        use std::os::unix::fs::PermissionsExt as _;

        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(FILE_NAME);
        let key = claim(Some(&path), PROD, site("my-site"), now()).key;
        let set_mode = |mode| {
            std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(mode))
                .expect("chmod");
        };
        set_mode(0o555);
        if std::fs::write(dir.path().join("probe"), "").is_ok() {
            // Root ignores directory permissions, so the failure cannot be made.
            set_mode(0o755);
            return;
        }

        let released = release(Some(&path), PROD, site("my-site"), now());
        set_mode(0o755);

        assert!(!released);
        assert_eq!(claim(Some(&path), PROD, site("my-site"), now()).key, key);
    }

    #[test]
    fn a_corrupt_file_reads_as_empty_and_is_replaced() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(FILE_NAME);
        std::fs::write(&path, "{ not json").expect("write corrupt file");

        let claimed = claim(Some(&path), PROD, site("my-site"), now());

        assert!(claimed.expires_at.is_some());
        assert_eq!(entries(&path).len(), 1);
    }

    #[test]
    fn a_key_that_cannot_be_saved_is_still_a_valid_key() {
        let dir = tempfile::tempdir().expect("tempdir");
        let blocker = dir.path().join("not-a-dir");
        std::fs::write(&blocker, "").expect("write blocker file");
        let path = blocker.join(FILE_NAME);

        for path in [Some(path.as_path()), None] {
            let claimed = claim(path, PROD, site("my-site"), now());
            assert_eq!(claimed.expires_at, None, "{path:?}");
            assert!(uuid::Uuid::parse_str(&claimed.key).is_ok(), "{path:?}");
        }
    }
}
