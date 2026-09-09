mod get;
mod lookup;
mod search;

use cli_engine::{GroupSpec, RuntimeGroupSpec};

pub(super) fn group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(
        GroupSpec::new("catalog", "Browse and resolve Shopping catalog products").with_long(
            "Search the live Shopping catalog, resolve known product IDs, and retrieve product details. \
             Every command requests the complete Shopping OAuth scope bundle so a catalog-to-checkout \
             workflow needs only one consent flow.",
        ),
    )
    .with_command(search::command())
    .with_command(lookup::command())
    .with_command(get::command())
}
