pub(super) mod complete;
mod create;
pub(super) mod get;
mod update;

use cli_engine::{GroupSpec, RuntimeGroupSpec};

pub(super) fn group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(
        GroupSpec::new("checkout", "Create and manage Shopping checkout sessions").with_long(
            "Create, update, and complete Shopping checkout sessions. Completion places a real \
             order. For a completed checkout, use `shopping order get` with the returned order ID; \
             `checkout get` is for open checkout sessions only.",
        ),
    )
    .with_command(create::command())
    .with_command(get::command())
    .with_command(update::command())
    .with_command(complete::command())
}
