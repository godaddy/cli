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
///
/// Unknown keys are rejected rather than ignored: a save rewrites the whole file
/// from what was parsed, so a misspelled or newer key that was ignored on the way
/// in would be silently deleted on the way out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StoreEntry {
    pub(crate) store_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) business_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) display_name: Option<String>,
}

/// The persisted selections, keyed by environment name.
///
/// Unknown top-level keys are rejected for the reason given on [`StoreEntry`]: a
/// misspelled table such as `[store.prod]` would otherwise parse as an empty
/// registry, and the next save would overwrite the file and discard what it held.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
    // a lock file just to find that out. Only a file that is really missing counts
    // as nothing saved: `Path::exists` also says "no" when the lookup itself fails
    // (an unsearchable directory, say), which would report success and leave the
    // selection in place.
    match path.try_exists() {
        Ok(true) => {}
        Ok(false) => return Ok(false),
        Err(error) => return Err(unreadable(path, &error)),
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
mod tests;
