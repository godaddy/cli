//! The persisted store selection for `gddy commerce`, and how a store id is
//! resolved from it.
//!
//! The selection lives in `commerce.toml` under the `gddy` config directory,
//! keyed by environment name, so a store chosen for `ote` never leaks into
//! `prod`. It is modeled on [`crate::pat`]'s registry: a small TOML file written
//! atomically with owner-only permissions, read through the same environment
//! names as the rest of the CLI.
//!
//! [`lookup`] resolves a store id in this order, and the first source that
//! supplies one wins:
//!
//! 1. the `--store-id` flag (passed in by the caller),
//! 2. `GDDY_STORE_ID_<ENV>`, for one environment,
//! 3. `GDDY_STORE_ID`, for any environment,
//! 4. the selection persisted for the active environment.
//!
//! The environment variables sit above the file so CI and scripts can pin a
//! store without touching a user's saved selection. Every id, whichever source
//! it comes from, must be a UUID, which is what the Stores API documents; a
//! value that is not one is rejected with the source named instead of being
//! passed on to fail later with a less useful error. A blank environment
//! variable counts as unset, but a blank `--store-id` is rejected, because
//! someone who typed the flag meant it.
//!
//! Saving and clearing are read-modify-write on one shared file, so each holds
//! an exclusive lock on `commerce.toml.lock` for the whole transaction. Without
//! it, two processes that read the same snapshot would each write back a copy
//! missing the other's change, even for different environments. Reads take no
//! lock: the file is replaced atomically, so a reader sees either the old or the
//! new contents, never a mix.

use std::{collections::BTreeMap, path::Path};

use cli_engine::{CliCoreError, Result};
use serde::{Deserialize, Serialize};

use crate::{environments, error::GddyError};

/// Environment variable that selects a store for any environment.
pub(crate) const STORE_ID_ENV_VAR: &str = "GDDY_STORE_ID";

const STORE_FILE_NAME: &str = "commerce.toml";

/// One persisted store selection. `business_id` and `display_name` are captured
/// from the API when the store is selected, so a later command can show more
/// than a bare id, but only `store_id` is needed to resolve a store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct StoreEntry {
    pub(crate) store_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) business_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) display_name: Option<String>,
}

/// The persisted selections, keyed by environment name.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct StoreRegistry {
    #[serde(default)]
    stores: BTreeMap<String, StoreEntry>,
}

impl StoreRegistry {
    /// Reads the registry, or an empty one if the file does not exist. A file
    /// that exists but cannot be read or parsed is an error, so a damaged
    /// selection is noticed instead of being treated as "no store selected".
    fn load(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(contents) => toml::from_str(&contents).map_err(|error| unreadable(path, &error)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(unreadable(path, &error)),
        }
    }

    /// Writes the registry atomically, with owner-only permissions on Unix.
    fn save(&self, path: &Path) -> Result<()> {
        let contents = toml::to_string_pretty(self).map_err(|error| {
            CliCoreError::message(format!("failed to serialize the store selection: {error}"))
        })?;
        cli_engine::fs::write_string_atomic(path, &contents)
    }
}

/// An exclusive lock on the registry, held until it is dropped. It is taken on
/// a sibling `.lock` file, not on `commerce.toml` itself, because saving replaces
/// that file with a new one and a lock on the old file would not protect the new.
struct RegistryLock {
    _file: std::fs::File,
}

impl RegistryLock {
    /// Waits for any other process holding the lock, then takes it. The wait is
    /// short (the critical section is one small file write), and the operating
    /// system releases the lock if the holder dies, so a crash cannot leave it
    /// stuck.
    fn acquire(registry: &Path) -> Result<Self> {
        let lock_path = registry.with_file_name(format!("{STORE_FILE_NAME}.lock"));
        let failed = |action: &str, error: std::io::Error| {
            CliCoreError::message(format!(
                "failed to {action} {}: {error}",
                lock_path.display()
            ))
        };
        if let Some(parent) = lock_path.parent() {
            create_private_dirs(parent)
                .map_err(|error| failed("create the directory for", error))?;
        }
        let mut options = std::fs::OpenOptions::new();
        options.create(true).write(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        let file = options
            .open(&lock_path)
            .map_err(|error| failed("open", error))?;
        file.lock().map_err(|error| failed("lock", error))?;
        Ok(Self { _file: file })
    }
}

/// Creates `dir` and any missing parents, owner-only (`0700`) on Unix, matching
/// what `write_string_atomic` does for directories it creates. A directory that
/// already exists is left as it is.
fn create_private_dirs(dir: &Path) -> std::io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        builder.mode(0o700);
    }
    builder.create(dir)
}

fn unreadable(path: &Path, error: &dyn std::fmt::Display) -> CliCoreError {
    GddyError::config(format!(
        "cannot read the saved store selection {}: {error}",
        path.display()
    ))
    .with_fix(
        "Fix or delete that file, then select a store again with: gddy commerce store use <store-id>",
    )
    .into_cli_error()
}

/// Where a resolved store id came from, so a command can say why a store is in
/// effect (for example an environment variable overriding what was saved).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StoreIdSource {
    /// The `--store-id` flag.
    Flag,
    /// `GDDY_STORE_ID_<ENV>`.
    EnvVarForEnv,
    /// `GDDY_STORE_ID`.
    EnvVar,
    /// The persisted selection.
    File,
}

impl StoreIdSource {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Flag => "flag",
            Self::EnvVarForEnv => "env-per-env",
            Self::EnvVar => "env",
            Self::File => "file",
        }
    }
}

/// A resolved store id and where it came from. `entry` is the full persisted
/// record, set only when `source` is [`StoreIdSource::File`]; a flag or an
/// environment variable carries nothing but the id.
#[derive(Debug, Clone)]
pub(crate) struct ResolvedStore {
    pub(crate) store_id: String,
    pub(crate) source: StoreIdSource,
    pub(crate) entry: Option<StoreEntry>,
}

fn env_var_for(env: &str) -> String {
    format!("{STORE_ID_ENV_VAR}_{}", environments::env_prefix(env))
}

/// Path to the saved selection, if a config directory can be found.
pub(crate) fn registry_path() -> Option<std::path::PathBuf> {
    dirs::config_dir().map(|dir| dir.join("gddy").join(STORE_FILE_NAME))
}

/// Checks that `value` is a store id (a UUID) and returns its canonical
/// lowercase hyphenated form, so one store is never saved under two spellings.
/// `origin` names where the value came from, for the error.
fn parse_store_id(value: &str, origin: &str, env: &str) -> Result<String> {
    uuid::Uuid::parse_str(value.trim())
        .map(|id| id.hyphenated().to_string())
        .map_err(|_| {
            GddyError::validation(format!("{origin} is not a store id: {value:?}"))
                .with_fix(format!(
                    "A store id is a UUID such as 6d1f0f4c-8a2e-4a52-9a5b-0f6a3c1d2e4b. {}",
                    super::find_your_store_id_hint(env)
                ))
                .into_cli_error()
        })
}

/// Resolves the store for `env` from `explicit` (the `--store-id` flag), then
/// the environment variables, then the saved selection. `None` means nothing is
/// selected, and the caller decides how to tell the user.
///
/// # Errors
///
/// A validation error if the value from the first source that supplies one is
/// not a store id, and a config error if the saved selection cannot be read.
pub(crate) fn lookup(env: &str, explicit: Option<&str>) -> Result<Option<ResolvedStore>> {
    resolve_with(
        env,
        explicit,
        |name| std::env::var(name).ok(),
        || match registry_path() {
            Some(path) => StoreRegistry::load(&path),
            None => Ok(StoreRegistry::default()),
        },
    )
}

/// [`lookup`] with its inputs injected so tests need neither the process
/// environment (`std::env::set_var` is `unsafe`, which this crate denies) nor a
/// real config directory. `registry` is only called when no flag or environment
/// variable supplied a store, so a saved selection is not read, or a damaged one
/// reported, when it would not be used.
fn resolve_with(
    env: &str,
    explicit: Option<&str>,
    get_env: impl Fn(&str) -> Option<String>,
    registry: impl FnOnce() -> Result<StoreRegistry>,
) -> Result<Option<ResolvedStore>> {
    if let Some(value) = explicit {
        return Ok(Some(ResolvedStore {
            store_id: parse_store_id(value, "--store-id", env)?,
            source: StoreIdSource::Flag,
            entry: None,
        }));
    }

    let per_env = env_var_for(env);
    for (name, source) in [
        (per_env.as_str(), StoreIdSource::EnvVarForEnv),
        (STORE_ID_ENV_VAR, StoreIdSource::EnvVar),
    ] {
        if let Some(value) = get_env(name).filter(|value| !value.trim().is_empty()) {
            return Ok(Some(ResolvedStore {
                store_id: parse_store_id(&value, name, env)?,
                source,
                entry: None,
            }));
        }
    }

    let Some(entry) = registry()?.stores.remove(env) else {
        return Ok(None);
    };
    let origin = format!("the saved store for the {env} environment");
    Ok(Some(ResolvedStore {
        store_id: parse_store_id(&entry.store_id, &origin, env)?,
        source: StoreIdSource::File,
        entry: Some(entry),
    }))
}

/// Saves `entry` as the selection for `env`, replacing any existing one and
/// leaving other environments' selections alone.
pub(crate) fn save_store(env: &str, entry: StoreEntry) -> Result<()> {
    save_store_at(&required_registry_path()?, env, entry)
}

/// Forgets the saved selection for `env`, returning whether there was one. It
/// does not touch `GDDY_STORE_ID` or `GDDY_STORE_ID_<ENV>`: those belong to the
/// caller's environment, not to this file.
pub(crate) fn clear_store(env: &str) -> Result<bool> {
    match registry_path() {
        Some(path) => clear_store_at(&path, env),
        None => Ok(false),
    }
}

fn required_registry_path() -> Result<std::path::PathBuf> {
    registry_path().ok_or_else(|| {
        GddyError::config("could not find a config directory to save the store selection in")
            .into_cli_error()
    })
}

fn save_store_at(path: &Path, env: &str, mut entry: StoreEntry) -> Result<()> {
    // Checked before anything is created or locked, so a bad id leaves no trace,
    // and canonicalized so the file holds the same spelling resolution produces.
    entry.store_id = parse_store_id(&entry.store_id, "the store id to save", env)?;
    let _lock = RegistryLock::acquire(path)?;
    let mut registry = StoreRegistry::load(path)?;
    registry.stores.insert(env.to_owned(), entry);
    registry.save(path)
}

fn clear_store_at(path: &Path, env: &str) -> Result<bool> {
    // Nothing saved means nothing to clear; do not create a config directory and
    // a lock file just to find that out.
    if !path.exists() {
        return Ok(false);
    }
    let _lock = RegistryLock::acquire(path)?;
    let mut registry = StoreRegistry::load(path)?;
    let existed = registry.stores.remove(env).is_some();
    if existed {
        registry.save(path)?;
    }
    Ok(existed)
}

#[cfg(test)]
mod tests {
    use super::*;

    const STORE_A: &str = "6d1f0f4c-8a2e-4a52-9a5b-0f6a3c1d2e4b";
    const STORE_B: &str = "a1e14cc2-29bd-4e99-a1ad-c426cb0266c1";
    const STORE_C: &str = "0c3b1a52-77de-4c0e-8f6b-1d2a9e4f5b60";

    fn entry(store_id: &str) -> StoreEntry {
        StoreEntry {
            store_id: store_id.to_owned(),
            business_id: None,
            display_name: None,
        }
    }

    fn registry_with(env: &str, store_id: &str) -> StoreRegistry {
        let mut registry = StoreRegistry::default();
        registry.stores.insert(env.to_owned(), entry(store_id));
        registry
    }

    fn no_env(_: &str) -> Option<String> {
        None
    }

    fn resolve(
        env: &str,
        explicit: Option<&str>,
        vars: &[(&str, &str)],
        registry: StoreRegistry,
    ) -> Result<Option<ResolvedStore>> {
        let vars: Vec<(String, String)> = vars
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect();
        resolve_with(
            env,
            explicit,
            move |name| {
                vars.iter()
                    .find(|(var, _)| var == name)
                    .map(|(_, value)| value.clone())
            },
            move || Ok(registry),
        )
    }

    fn resolved(
        env: &str,
        explicit: Option<&str>,
        vars: &[(&str, &str)],
        registry: StoreRegistry,
    ) -> ResolvedStore {
        resolve(env, explicit, vars, registry)
            .expect("resolves")
            .expect("a store is selected")
    }

    fn message(error: &CliCoreError) -> String {
        cli_engine::build_error_envelope(error, "commerce")
            .error
            .map(|e| e.message)
            .unwrap_or_default()
    }

    fn fix(error: &CliCoreError) -> String {
        cli_engine::build_error_envelope(error, "commerce")
            .fix
            .unwrap_or_default()
    }

    #[test]
    fn the_env_var_name_is_the_uppercased_environment() {
        assert_eq!(env_var_for("prod"), "GDDY_STORE_ID_PROD");
        assert_eq!(env_var_for("ote"), "GDDY_STORE_ID_OTE");
        assert_eq!(env_var_for("my-env"), "GDDY_STORE_ID_MY_ENV");
    }

    #[test]
    fn the_flag_beats_every_other_source() {
        let store = resolved(
            "ote",
            Some(STORE_A),
            &[("GDDY_STORE_ID_OTE", STORE_B), ("GDDY_STORE_ID", STORE_B)],
            registry_with("ote", STORE_C),
        );
        assert_eq!(store.store_id, STORE_A);
        assert_eq!(store.source, StoreIdSource::Flag);
        assert!(store.entry.is_none());
    }

    #[test]
    fn the_per_environment_variable_beats_the_bare_variable_and_the_file() {
        let store = resolved(
            "ote",
            None,
            &[("GDDY_STORE_ID_OTE", STORE_A), ("GDDY_STORE_ID", STORE_B)],
            registry_with("ote", STORE_C),
        );
        assert_eq!(store.store_id, STORE_A);
        assert_eq!(store.source, StoreIdSource::EnvVarForEnv);
    }

    #[test]
    fn a_per_environment_variable_for_another_environment_is_ignored() {
        let store = resolved(
            "ote",
            None,
            &[("GDDY_STORE_ID_PROD", STORE_A)],
            registry_with("ote", STORE_C),
        );
        assert_eq!(store.store_id, STORE_C);
        assert_eq!(store.source, StoreIdSource::File);
    }

    #[test]
    fn the_bare_variable_beats_the_file() {
        let store = resolved(
            "ote",
            None,
            &[("GDDY_STORE_ID", STORE_B)],
            registry_with("ote", STORE_C),
        );
        assert_eq!(store.store_id, STORE_B);
        assert_eq!(store.source, StoreIdSource::EnvVar);
    }

    #[test]
    fn the_file_is_used_when_nothing_else_is_set_and_carries_its_entry() {
        let mut registry = StoreRegistry::default();
        registry.stores.insert(
            "ote".to_owned(),
            StoreEntry {
                store_id: STORE_A.to_owned(),
                business_id: Some("biz-1".to_owned()),
                display_name: Some("My Shop".to_owned()),
            },
        );
        let store = resolved("ote", None, &[], registry);
        assert_eq!(store.source, StoreIdSource::File);
        let entry = store.entry.expect("the file entry is carried");
        assert_eq!(entry.business_id.as_deref(), Some("biz-1"));
        assert_eq!(entry.display_name.as_deref(), Some("My Shop"));
    }

    #[test]
    fn nothing_selected_resolves_to_none() {
        let store = resolve("ote", None, &[], StoreRegistry::default()).expect("resolves");
        assert!(store.is_none());
    }

    #[test]
    fn a_selection_for_another_environment_is_not_used() {
        let store = resolve("ote", None, &[], registry_with("prod", STORE_A)).expect("resolves");
        assert!(store.is_none(), "{store:?}");
    }

    #[test]
    fn a_blank_environment_variable_counts_as_unset() {
        let store = resolved(
            "ote",
            None,
            &[("GDDY_STORE_ID_OTE", "  "), ("GDDY_STORE_ID", "")],
            registry_with("ote", STORE_C),
        );
        assert_eq!(store.store_id, STORE_C);
        assert_eq!(store.source, StoreIdSource::File);
    }

    #[test]
    fn a_blank_flag_is_rejected_not_skipped() {
        for blank in ["", "   "] {
            let error = resolve(
                "ote",
                Some(blank),
                &[("GDDY_STORE_ID", STORE_B)],
                registry_with("ote", STORE_C),
            )
            .expect_err("a blank --store-id is an error");
            assert!(
                message(&error).contains("--store-id"),
                "{}",
                message(&error)
            );
        }
    }

    #[test]
    fn an_invalid_id_is_rejected_and_names_where_it_came_from() {
        let from_flag = resolve("ote", Some("nope"), &[], StoreRegistry::default())
            .expect_err("flag is invalid");
        assert!(message(&from_flag).contains("--store-id"));
        assert!(
            fix(&from_flag).contains("Your stores"),
            "{}",
            fix(&from_flag)
        );

        let from_env = resolve(
            "ote",
            None,
            &[("GDDY_STORE_ID_OTE", "nope")],
            StoreRegistry::default(),
        )
        .expect_err("env var is invalid");
        assert!(message(&from_env).contains("GDDY_STORE_ID_OTE"));

        let from_bare = resolve(
            "ote",
            None,
            &[("GDDY_STORE_ID", "nope")],
            StoreRegistry::default(),
        )
        .expect_err("bare env var is invalid");
        assert!(message(&from_bare).contains("GDDY_STORE_ID"));

        let from_file = resolve("ote", None, &[], registry_with("ote", "nope"))
            .expect_err("saved selection is invalid");
        assert!(message(&from_file).contains("saved store"));
    }

    /// The first source that supplies a value decides, so an invalid value is
    /// reported even though a later source would have been valid.
    #[test]
    fn an_invalid_higher_priority_value_is_not_skipped_for_a_valid_lower_one() {
        let error = resolve(
            "ote",
            None,
            &[("GDDY_STORE_ID_OTE", "nope"), ("GDDY_STORE_ID", STORE_B)],
            registry_with("ote", STORE_C),
        )
        .expect_err("the per-env value is invalid");
        assert!(message(&error).contains("GDDY_STORE_ID_OTE"));
    }

    #[test]
    fn ids_are_canonicalized_to_lowercase_hyphenated_form() {
        let upper = STORE_A.to_uppercase();
        let braced = format!("{{{STORE_A}}}");
        let simple = STORE_A.replace('-', "");
        for spelling in [
            upper.as_str(),
            braced.as_str(),
            simple.as_str(),
            " 6d1f0f4c-8a2e-4a52-9a5b-0f6a3c1d2e4b ",
        ] {
            let store = resolved("ote", Some(spelling), &[], StoreRegistry::default());
            assert_eq!(store.store_id, STORE_A, "{spelling:?}");
        }
    }

    /// A flag or environment variable that supplies a store must not read the
    /// saved selection, so a damaged file is not reported when it would not be
    /// used.
    #[test]
    fn the_saved_selection_is_not_read_when_a_flag_or_variable_supplies_a_store() {
        let reads = std::cell::Cell::new(0_u32);
        let counting_registry = || {
            reads.set(reads.get() + 1);
            Ok(StoreRegistry::default())
        };

        let from_flag =
            resolve_with("ote", Some(STORE_A), no_env, counting_registry).expect("resolves");
        assert_eq!(from_flag.expect("selected").store_id, STORE_A);

        let from_env = resolve_with(
            "ote",
            None,
            |name| (name == STORE_ID_ENV_VAR).then(|| STORE_B.to_owned()),
            counting_registry,
        )
        .expect("resolves");
        assert_eq!(from_env.expect("selected").store_id, STORE_B);

        assert_eq!(
            reads.get(),
            0,
            "the registry was read though it was not needed"
        );

        // The counter is live: it does count a read when nothing else supplies a store.
        let none = resolve_with("ote", None, no_env, counting_registry).expect("resolves");
        assert!(none.is_none());
        assert_eq!(reads.get(), 1);
    }

    #[test]
    fn a_registry_error_is_returned_when_it_is_needed() {
        let error = resolve_with("ote", None, no_env, || {
            Err(CliCoreError::message("registry unreadable"))
        })
        .expect_err("the registry error propagates");
        assert!(message(&error).contains("registry unreadable"));
    }

    #[test]
    fn saving_and_loading_round_trips_an_entry() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("gddy").join(STORE_FILE_NAME);
        save_store_at(
            &path,
            "ote",
            StoreEntry {
                store_id: STORE_A.to_owned(),
                business_id: Some("biz-1".to_owned()),
                display_name: Some("My Shop".to_owned()),
            },
        )
        .expect("save creates the directory and the file");

        let loaded = StoreRegistry::load(&path).expect("load");
        let entry = loaded.stores.get("ote").expect("entry present");
        assert_eq!(entry.store_id, STORE_A);
        assert_eq!(entry.business_id.as_deref(), Some("biz-1"));
        assert_eq!(entry.display_name.as_deref(), Some("My Shop"));
    }

    #[test]
    fn saving_replaces_one_environments_entry_and_keeps_the_others() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(STORE_FILE_NAME);
        save_store_at(&path, "ote", entry(STORE_A)).expect("save ote");
        save_store_at(&path, "prod", entry(STORE_B)).expect("save prod");
        save_store_at(&path, "ote", entry(STORE_C)).expect("replace ote");

        let loaded = StoreRegistry::load(&path).expect("load");
        assert_eq!(loaded.stores["ote"].store_id, STORE_C);
        assert_eq!(loaded.stores["prod"].store_id, STORE_B);
    }

    #[test]
    fn two_environments_resolve_to_their_own_stores_through_the_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(STORE_FILE_NAME);
        save_store_at(&path, "ote", entry(STORE_A)).expect("save ote");
        save_store_at(&path, "prod", entry(STORE_B)).expect("save prod");

        let for_env = |env: &str| {
            resolve_with(env, None, no_env, || StoreRegistry::load(&path))
                .expect("resolves")
                .expect("selected")
                .store_id
        };
        assert_eq!(for_env("ote"), STORE_A);
        assert_eq!(for_env("prod"), STORE_B);
    }

    #[test]
    fn clearing_removes_only_the_named_environment() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(STORE_FILE_NAME);
        save_store_at(&path, "ote", entry(STORE_A)).expect("save ote");
        save_store_at(&path, "prod", entry(STORE_B)).expect("save prod");

        assert!(clear_store_at(&path, "ote").expect("clear"));

        let loaded = StoreRegistry::load(&path).expect("load");
        assert!(!loaded.stores.contains_key("ote"));
        assert!(loaded.stores.contains_key("prod"));
    }

    #[test]
    fn clearing_something_that_is_not_there_reports_false_and_writes_nothing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(STORE_FILE_NAME);
        assert!(!clear_store_at(&path, "ote").expect("clear on a missing file"));
        assert!(!path.exists(), "clearing must not create the file");

        save_store_at(&path, "prod", entry(STORE_B)).expect("save prod");
        assert!(!clear_store_at(&path, "ote").expect("clear a missing entry"));
    }

    #[test]
    fn a_missing_file_is_an_empty_registry() {
        let dir = tempfile::tempdir().expect("tempdir");
        let registry = StoreRegistry::load(&dir.path().join(STORE_FILE_NAME)).expect("load");
        assert!(registry.stores.is_empty());
    }

    #[test]
    fn a_corrupt_file_is_an_error_that_names_the_file_and_is_not_overwritten() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(STORE_FILE_NAME);
        std::fs::write(&path, "stores = [not toml").expect("seed");

        let error = StoreRegistry::load(&path).expect_err("corrupt file is an error");
        assert!(message(&error).contains(&path.display().to_string()));
        assert!(fix(&error).contains("store use"), "{}", fix(&error));

        save_store_at(&path, "ote", entry(STORE_A)).expect_err("saving over a corrupt file fails");
        assert_eq!(
            std::fs::read_to_string(&path).expect("read"),
            "stores = [not toml",
            "a corrupt file must be left for the user to fix"
        );
    }

    /// A save used to write whatever it was given, so an uppercase or braced id
    /// stayed in that spelling and an invalid one was saved and then failed on
    /// the next lookup. The save now enforces what resolution enforces.
    #[test]
    fn saving_canonicalizes_the_id_so_the_file_holds_one_spelling() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(STORE_FILE_NAME);
        let braced = format!("{{{}}}", STORE_A.to_uppercase());
        save_store_at(&path, "ote", entry(&braced)).expect("a valid id in any spelling saves");

        let loaded = StoreRegistry::load(&path).expect("load");
        assert_eq!(loaded.stores["ote"].store_id, STORE_A);
        assert!(
            std::fs::read_to_string(&path)
                .expect("read")
                .contains(STORE_A),
            "the file should contain the canonical spelling"
        );
    }

    #[test]
    fn saving_an_invalid_id_fails_and_changes_nothing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("gddy").join(STORE_FILE_NAME);

        let error = save_store_at(&path, "ote", entry("nope")).expect_err("not a store id");
        assert!(
            message(&error).contains("not a store id"),
            "{}",
            message(&error)
        );
        assert!(!path.exists(), "a rejected save must not create the file");
        assert!(
            !path
                .with_file_name(format!("{STORE_FILE_NAME}.lock"))
                .exists(),
            "a rejected save must not create a lock file or directory"
        );

        save_store_at(&path, "prod", entry(STORE_B)).expect("save prod");
        let before = std::fs::read_to_string(&path).expect("read");
        save_store_at(&path, "ote", entry("")).expect_err("a blank id is rejected too");
        assert_eq!(std::fs::read_to_string(&path).expect("read"), before);
    }

    #[test]
    fn saving_keeps_business_id_and_display_name() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(STORE_FILE_NAME);
        let upper = STORE_A.to_uppercase();
        save_store_at(
            &path,
            "ote",
            StoreEntry {
                store_id: upper,
                business_id: Some("biz-1".to_owned()),
                display_name: Some("My Shop".to_owned()),
            },
        )
        .expect("save");
        let loaded = StoreRegistry::load(&path).expect("load");
        assert_eq!(loaded.stores["ote"].business_id.as_deref(), Some("biz-1"));
        assert_eq!(
            loaded.stores["ote"].display_name.as_deref(),
            Some("My Shop")
        );
    }

    /// The race the lock exists for: with no lock, two writers that read the same
    /// snapshot each write back a registry missing the other's entry. Each thread
    /// saves its own environment 25 times; if any update were lost, an
    /// environment's final entry would be missing or stale.
    #[test]
    fn concurrent_saves_for_different_environments_lose_no_updates() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = std::sync::Arc::new(dir.path().join("gddy").join(STORE_FILE_NAME));
        let stores = [STORE_A, STORE_B, STORE_C];
        let handles: Vec<_> = stores
            .iter()
            .enumerate()
            .map(|(index, store)| {
                let path = std::sync::Arc::clone(&path);
                let store = (*store).to_owned();
                std::thread::spawn(move || {
                    for _ in 0..25 {
                        save_store_at(&path, &format!("env{index}"), entry(&store))
                            .expect("save under contention");
                    }
                })
            })
            .collect();
        for handle in handles {
            handle.join().expect("thread finished");
        }

        let loaded = StoreRegistry::load(&path).expect("load");
        for (index, store) in stores.iter().enumerate() {
            assert_eq!(
                loaded
                    .stores
                    .get(&format!("env{index}"))
                    .map(|e| e.store_id.as_str()),
                Some(*store),
                "env{index}'s entry was lost to a concurrent write: {loaded:?}"
            );
        }
    }

    #[test]
    fn concurrent_saves_and_clears_lose_no_updates() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = std::sync::Arc::new(dir.path().join(STORE_FILE_NAME));
        save_store_at(&path, "keep", entry(STORE_A)).expect("seed");

        let writers: Vec<_> = (0..4)
            .map(|index| {
                let path = std::sync::Arc::clone(&path);
                std::thread::spawn(move || {
                    for round in 0..20 {
                        let env = format!("tmp{index}");
                        save_store_at(&path, &env, entry(STORE_B)).expect("save");
                        if round % 2 == 0 {
                            clear_store_at(&path, &env).expect("clear");
                        }
                    }
                })
            })
            .collect();
        for writer in writers {
            writer.join().expect("thread finished");
        }

        let loaded = StoreRegistry::load(&path).expect("load");
        assert_eq!(
            loaded.stores.get("keep").map(|e| e.store_id.as_str()),
            Some(STORE_A),
            "an entry nobody touched was lost: {loaded:?}"
        );
        for index in 0..4 {
            // Round 19 is odd, so each thread's last action was a save.
            assert!(
                loaded.stores.contains_key(&format!("tmp{index}")),
                "tmp{index} was lost: {loaded:?}"
            );
        }
    }

    /// The lock is a real exclusion, not a no-op: while one handle holds it,
    /// another cannot take it, and it is released when the holder is dropped.
    #[test]
    fn the_registry_lock_excludes_a_second_holder_until_released() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(STORE_FILE_NAME);
        let lock_path = path.with_file_name(format!("{STORE_FILE_NAME}.lock"));

        let held = RegistryLock::acquire(&path).expect("first holder");
        let other = std::fs::OpenOptions::new()
            .write(true)
            .open(&lock_path)
            .expect("open the lock file");
        assert!(other.try_lock().is_err(), "a second holder got the lock");

        drop(held);
        other.try_lock().expect("the lock is free once released");
    }

    #[test]
    fn clearing_with_no_file_creates_neither_the_file_nor_a_lock() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("gddy").join(STORE_FILE_NAME);
        assert!(!clear_store_at(&path, "ote").expect("clear"));
        assert!(
            !dir.path().join("gddy").exists(),
            "no directory should be created"
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_lock_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("gddy").join(STORE_FILE_NAME);
        save_store_at(&path, "ote", entry(STORE_A)).expect("save");
        let lock = path.with_file_name(format!("{STORE_FILE_NAME}.lock"));
        let mode = std::fs::metadata(&lock)
            .expect("lock file exists")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn the_saved_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("gddy").join(STORE_FILE_NAME);
        save_store_at(&path, "ote", entry(STORE_A)).expect("save");
        let mode = std::fs::metadata(&path)
            .expect("metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}
