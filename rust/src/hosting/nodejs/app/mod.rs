mod create;
mod delete;
mod get;
mod list;
mod update;

use cli_engine::{GroupSpec, RuntimeGroupSpec};

pub(super) fn group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(GroupSpec::new(
        "app",
        "Create, inspect, update, and delete Node.js hosting apps",
    ))
    .with_command(list::command())
    .with_command(get::command())
    .with_command(create::command())
    .with_command(update::command())
    .with_command(delete::command())
}
