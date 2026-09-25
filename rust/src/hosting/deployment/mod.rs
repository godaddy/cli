mod get;
mod list;
mod publish;

use cli_engine::{GroupSpec, RuntimeGroupSpec};

pub(super) fn group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(
        GroupSpec::new("deployment", "Manage application deployments").with_long(
            "List, inspect, and trigger deployments for a hosting application. \
             Deployments are triggered by `publish` and poll-able via `get`. \
             `publish` deploys to PUBLISH; to update the PREVIEW environment \
             instead, use `hosting source upload` or `hosting source github`.",
        ),
    )
    .with_command(list::command())
    .with_command(get::command())
    .with_command(publish::command())
}
