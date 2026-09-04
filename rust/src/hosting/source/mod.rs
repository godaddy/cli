mod import;
mod status;

use cli_engine::{GroupSpec, RuntimeGroupSpec};

pub(super) fn group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(
        GroupSpec::new("source", "Manage application source code").with_long(
            "Import source code from GitHub and check import status. \
             After a successful import, use `hosting deployment publish` to deploy.",
        ),
    )
    .with_command(import::command())
    .with_command(status::command())
}
