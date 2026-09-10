pub(super) mod get;

use cli_engine::{GroupSpec, RuntimeGroupSpec};

pub(super) fn group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(GroupSpec::new("order", "Read completed Shopping orders"))
        .with_command(get::command())
}
