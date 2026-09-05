mod create;
mod delete;
mod list;
mod sync;
mod update;

use cli_engine::{GroupSpec, RuntimeGroupSpec};

pub(super) fn group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(
        GroupSpec::new("secrets", "Manage application secrets").with_long(
            "Create, update, delete, and list secrets for a hosting application. \
             Secret values are write-only — list returns names only. \
             Use --variant to target PREVIEW or PUBLISH (defaults to PREVIEW for writes).",
        ),
    )
    .with_command(list::command())
    .with_command(create::command())
    .with_command(update::command())
    .with_command(delete::command())
    .with_command(sync::command())
}
