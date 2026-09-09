mod complete;
mod create;
mod get;
mod update;

use cli_engine::{GroupSpec, RuntimeGroupSpec};

pub(super) fn group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(
        GroupSpec::new("checkout", "Create and manage Shopping checkout sessions").with_long(
            "Create, update, and complete UCP checkout sessions. Completion places a real order. \
             A completed checkout must be followed with `shopping order get`, not `checkout get`, \
             because the current service reconstructs checkout reads from its open basket.",
        ),
    )
    .with_command(create::command())
    .with_command(get::command())
    .with_command(update::command())
    .with_command(complete::command())
}
