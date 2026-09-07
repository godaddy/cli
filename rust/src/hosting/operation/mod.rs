mod get;

use cli_engine::{GroupSpec, RuntimeGroupSpec};

pub(super) fn group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(
        GroupSpec::new("operation", "Poll async operations").with_long(
            "Poll the status of async operations returned by hosting commands. \
             Today only `hosting app create` returns one; `hosting deployment publish` has its \
             own poller at `hosting deployment get`.",
        ),
    )
    .with_command(get::command())
}
