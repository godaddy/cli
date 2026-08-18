//! Shared test-only helpers for env-var-touching tests across this module's
//! submodules — see [`ENV_LOCK`]'s own doc for why serialization is needed.

use std::sync::Mutex;

/// Serializes every test that touches real process env vars, so parallel
/// test threads can't observe each other's GDDY_* overrides.
pub(super) static ENV_LOCK: Mutex<()> = Mutex::new(());

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
