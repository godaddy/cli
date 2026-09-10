mod branches;
mod repos;
mod status;

use cli_engine::{GroupSpec, RuntimeGroupSpec};

pub(super) fn group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(
        GroupSpec::new("github", "Manage GitHub integration").with_long(
            "Inspect your GitHub connection, browse accessible repositories, \
             and list branches. Connect GitHub at godaddy.com before using these commands.",
        ),
    )
    .with_command(status::command())
    .with_command(repos::command())
    .with_command(branches::command())
}
