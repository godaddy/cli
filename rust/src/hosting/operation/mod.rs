mod get;

use cli_engine::{GroupSpec, RuntimeGroupSpec};

pub(super) fn group(mhwp: bool) -> RuntimeGroupSpec {
    // Only WordPress sites have a publish state; name it only while the flag is on.
    let long = if mhwp {
        "Poll the status of async operations returned by hosting commands: \
         `hosting app create`, and `hosting app publish` and `hosting app unpublish` for a \
         WordPress site. Publishing a Node.js app, with `hosting app publish` or \
         `hosting deployment publish`, starts a deployment, polled at `hosting deployment get`."
    } else {
        "Poll the status of async operations returned by hosting commands. \
         Today only `hosting app create` returns one; `hosting deployment publish` has its \
         own poller at `hosting deployment get`."
    };
    RuntimeGroupSpec::new(
        GroupSpec::new("operation", "Poll async operations")
            .with_alias("operations")
            .with_long(long),
    )
    .with_command(get::command(mhwp))
}
