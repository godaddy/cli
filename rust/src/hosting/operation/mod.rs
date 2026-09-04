mod get;

use cli_engine::{GroupSpec, RuntimeGroupSpec};

pub(super) fn group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(
        GroupSpec::new("operation", "Poll async operations").with_long(
            "Poll the status of async operations returned by hosting commands. \
             Operations are created by `hosting app create` and `hosting deployment publish`.",
        ),
    )
    .with_command(get::command())
}
