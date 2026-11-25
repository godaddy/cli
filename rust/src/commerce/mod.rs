//! `gddy commerce` — merchant-facing commands over the Commerce Catalog and
//! Stores APIs.
//!
//! The group is in beta until the commerce team signs off that the commands are
//! ready, so it stays hidden at the default release stage.

mod catalog;
mod client;
pub(crate) mod store;

use cli_engine::{GroupSpec, Module, RuntimeGroupSpec, Stage};

pub(crate) fn module() -> Module {
    Module::new("Commerce", |_ctx| {
        RuntimeGroupSpec::new(
            GroupSpec::new("commerce", "Manage your GoDaddy Commerce store").with_long(
                "Work with your GoDaddy Commerce store.\n\
             \n\
             \u{2022} store  \u{2014} choose the store the other commands work \
             on, and see its details\n\
             \n\
             Start with `gddy commerce store use <store-id>`. Your store id is \
             on the \"Your stores\" page of the GoDaddy developer portal.",
            ),
        )
        .with_group(store::group())
    })
    .with_feature_flag("commerce", Stage::Beta)
}
