mod github;
mod status;
mod upload;

use cli_engine::{GroupSpec, RuntimeGroupSpec};

pub(super) fn group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(
        GroupSpec::new("source", "Manage application source code").with_long(
            "Upload source code from a local zip archive or import from GitHub, \
             then check import status. After a successful import, use \
             `hosting deployment publish` to deploy.",
        ),
    )
    .with_command(upload::command())
    .with_command(status::command())
    .with_command(github::command())
}
