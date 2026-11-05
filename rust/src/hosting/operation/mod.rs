mod get;

use cli_engine::{GroupSpec, RuntimeGroupSpec};

pub(super) fn group(mhwp: bool) -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(
        GroupSpec::new("operation", "Poll async operations")
            .with_alias("operations")
            .with_long("Commands for monitoring asynchronous hosting operations."),
    )
    .with_command(get::command(mhwp))
}
