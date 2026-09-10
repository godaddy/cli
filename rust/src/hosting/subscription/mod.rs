mod attach;
mod get;
mod list;

use cli_engine::{GroupSpec, RuntimeGroupSpec};

pub(super) fn group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(
        GroupSpec::new("subscription", "Manage hosting plan subscriptions").with_long(
            "List available hosting plan subscriptions and attach one to an application. \
             A subscription represents a hosting plan (resources, billing) that backs \
             the application's runtime.",
        ),
    )
    .with_command(list::command())
    .with_command(get::command())
    .with_command(attach::command())
}
