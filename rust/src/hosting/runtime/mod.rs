mod get;

use cli_engine::{GroupSpec, RuntimeGroupSpec};

pub(super) fn group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(
        GroupSpec::new("runtime", "View application runtime configuration").with_long(
            "Get the runtime name and version for a hosting application \
             (e.g. runtime=nodejs, version=22).",
        ),
    )
    .with_command(get::command())
}
