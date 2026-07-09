//! Helper so every suggested next-step command reads exactly as the user would
//! type it (e.g. `gddy domain quote <domain>`, not `domain quote <domain>`).
//!
//! `cli_engine::NextAction` is a generic, app-agnostic type with no concept of a
//! binary name, so the `gddy` prefix is applied here in the main repo rather than
//! in the (separately versioned) `cli-engine` crate.

use cli_engine::NextAction;

use crate::environments::APP_ID;

pub(crate) fn next_action(
    command: impl Into<String>,
    description: impl Into<String>,
) -> NextAction {
    NextAction::new(format!("{APP_ID} {}", command.into()), description)
}
