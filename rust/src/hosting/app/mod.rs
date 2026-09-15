mod create;
mod delete;
mod get;
mod list;
mod restart;
mod status;
mod update;

use cli_engine::{GroupSpec, RuntimeGroupSpec};

pub(super) fn group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(
        GroupSpec::new(
            "app",
            "Create, inspect, update, and delete hosting applications",
        )
        .with_long(
            "Work with hosting applications. Use --app-type on list and create to \
             specify the product type (currently NODEJS).",
        ),
    )
    .with_command(list::command())
    .with_command(get::command())
    .with_command(create::command())
    .with_command(update::command())
    .with_command(delete::command())
    .with_command(status::command())
    .with_command(restart::command())
}
