pub(super) mod get;
pub(super) mod lookup;
pub(super) mod search;

use cli_engine::{GroupSpec, RuntimeGroupSpec};

pub(super) fn group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(
        GroupSpec::new("catalog", "Explore GoDaddy's product catalog")
            .with_long("Search GoDaddy's product catalog and retrieve product details."),
    )
    .with_command(search::command())
    .with_command(lookup::command())
    .with_command(get::command())
}
