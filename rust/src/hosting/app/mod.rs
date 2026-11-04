mod common;
mod create;
mod delete;
mod get;
mod list;
mod publish;
mod restart;
mod status;
mod unpublish;
mod update;

use cli_engine::{GroupSpec, RuntimeGroupSpec};

use crate::hosting::common::supported_app_types;

pub(super) fn group(mhwp: bool) -> RuntimeGroupSpec {
    let mut long = format!(
        "Work with hosting applications. Use --app-type on list and create to \
         specify the application type (currently {}).",
        supported_app_types(mhwp)
    );
    if mhwp {
        long.push_str(
            " `publish` takes any app live; `unpublish` takes a WordPress (MHWP) site back \
             to a placeholder page.",
        );
    }
    let group = RuntimeGroupSpec::new(
        GroupSpec::new(
            "app",
            "Create, inspect, update, and delete hosting applications",
        )
        .with_alias("apps")
        .with_long(long),
    )
    .with_command(list::command(mhwp))
    .with_command(get::command())
    .with_command(create::command(mhwp))
    .with_command(update::command())
    .with_command(delete::command())
    .with_command(status::command())
    .with_command(restart::command());
    if mhwp {
        // Both name WordPress, so they ship with its flag; Node.js keeps
        // `deployment publish` either way.
        group
            .with_command(publish::command())
            .with_command(unpublish::command())
    } else {
        group
    }
}
