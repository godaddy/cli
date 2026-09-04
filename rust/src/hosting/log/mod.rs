mod list;

use cli_engine::{GroupSpec, RuntimeGroupSpec};

pub(super) fn group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(GroupSpec::new("log", "View application logs").with_long(
        "Retrieve log entries for a hosting application. \
             Use --target to select the environment, --since to set a time window, \
             and --level/--source to filter.",
    ))
    .with_command(list::command())
}
