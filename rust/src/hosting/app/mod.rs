mod create;
mod delete;
mod get;
mod list;
mod restart;
mod status;
mod update;

use cli_engine::{GroupSpec, RuntimeGroupSpec};

use crate::hosting::common::supported_app_types;

pub(super) fn group(mhwp: bool) -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(
        GroupSpec::new(
            "app",
            "Create, inspect, update, and delete hosting applications",
        )
        .with_long(format!(
            "Work with hosting applications. Use --app-type on list and create to \
             specify the product type (currently {}).",
            supported_app_types(mhwp)
        )),
    )
    .with_command(list::command(mhwp))
    .with_command(get::command())
    .with_command(create::command(mhwp))
    .with_command(update::command())
    .with_command(delete::command())
    .with_command(status::command())
    .with_command(restart::command())
}
