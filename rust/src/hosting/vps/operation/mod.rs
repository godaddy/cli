mod get;

use cli_engine::{GroupSpec, RuntimeGroupSpec};

pub(super) fn group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(GroupSpec::new(
        "operation",
        "Check start, stop and restart operations",
    ))
    .with_command(get::command())
}
