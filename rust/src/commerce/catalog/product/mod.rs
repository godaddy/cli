//! `gddy commerce catalog product` — the products in a store's catalog.

mod archive;
mod create;
mod fields;
mod get;
mod list;
mod output;
mod result;
mod update;

use cli_engine::{GroupSpec, RuntimeGroupSpec};

pub(super) fn group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(
        GroupSpec::new("product", "Manage products in your catalog").with_long(
            "A product is an item offered for sale in your store, which may \
            have multiple variants. It can be thought of as a group of SKUs.",
        ),
    )
    .with_command(list::command())
    .with_command(get::command())
    .with_command(create::command())
    .with_command(update::command())
    .with_command(archive::command())
}

#[cfg(test)]
mod tests;
