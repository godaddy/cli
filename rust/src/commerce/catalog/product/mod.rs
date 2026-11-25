//! `gddy commerce catalog product` — the products in a store's catalog.

mod get;
mod list;
mod output;

use cli_engine::{GroupSpec, RuntimeGroupSpec};

pub(super) fn group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(
        GroupSpec::new("product", "Manage products in your catalog").with_long(
            "A product is an item offered for sale in your store, which may be \
            have multiple variants. It can be thought of as a group of SKUs.",
        ),
    )
    .with_command(list::command())
    .with_command(get::command())
}
