mod attach;
mod detach;
mod get;
mod list;

use cli_engine::{GroupSpec, RuntimeGroupSpec};

pub(super) fn group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(
        GroupSpec::new("domain", "Manage domains attached to an application").with_long(
            "List, inspect, attach, and detach domains for a hosting application. \
             Domains must be registered and have DNS pointing to GoDaddy hosting.",
        ),
    )
    .with_command(list::command())
    .with_command(get::command())
    .with_command(attach::command())
    .with_command(detach::command())
}
