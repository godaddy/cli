mod get;

use cli_engine::{GroupSpec, RuntimeGroupSpec};

pub(super) fn group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(
        GroupSpec::new("runtime", "View application runtime configuration").with_long(
            "Get the runtime configuration for a hosting application, \
             including environment variables, entry point, and resource limits.",
        ),
    )
    .with_command(get::command())
}
