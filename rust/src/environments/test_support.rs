//! Shared test-only helpers for env-var-touching tests — see [`ENV_LOCK`]'s
//! own doc for why serialization is needed.

use tokio::sync::Mutex;

/// Serializes every test that touches real process env vars, so parallel
/// test threads can't observe each other's GDDY_* overrides.
///
/// `pub(crate)`, not `pub(super)`: `std::env::set_var` is process-global, so
/// any test anywhere in this crate that resolves a real environment's config
/// (built-in or custom — the override applies regardless) can observe a
/// `GDDY_*`-mutating test's in-flight value unless it also holds this lock.
/// Grep for `ENV_LOCK` before adding a test that calls `environments::resolve`
/// (or anything built on it, like `Cli::run` against a real `--env`) with an
/// assertion sensitive to the resolved config being valid.
///
/// A `tokio::sync::Mutex`, not `std::sync::Mutex`: several callers are
/// `#[tokio::test]`s that hold the guard across `cli.run(...).await` (a plain
/// std guard held across an await point is `clippy::await_holding_lock`,
/// denied here via `-D warnings`). Sync `#[test]`s use
/// [`blocking_lock`](Mutex::blocking_lock) instead of `.lock().await`.
pub(crate) static ENV_LOCK: Mutex<()> = Mutex::const_new(());

/// RAII guard that sets an env var and restores it to its prior state on
/// drop — removing it if it wasn't already set, or putting the original
/// value back if it was — even if a test panics. Restoring rather than
/// unconditionally removing keeps a var a developer happens to already
/// have set in their shell from leaking into the rest of the test run.
pub(super) struct EnvGuard {
    key: &'static str,
    prior: Option<String>,
}

impl EnvGuard {
    pub(super) fn set(key: &'static str, value: &str) -> Self {
        let prior = std::env::var(key).ok();
        // SAFETY: caller holds ENV_LOCK.
        #[allow(unsafe_code)]
        unsafe {
            std::env::set_var(key, value);
        }
        Self { key, prior }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        // SAFETY: caller holds ENV_LOCK; restore on any exit incl. panic.
        #[allow(unsafe_code)]
        unsafe {
            match &self.prior {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }
}
