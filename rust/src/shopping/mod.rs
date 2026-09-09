pub mod client;

mod catalog;
mod checkout;
mod common;
mod order;

use cli_engine::{GroupSpec, Module, RuntimeGroupSpec};

use crate::shopping::catalog::search::register_human_view;

use crate::scopes::{SHOPPING_CATALOG_READ, SHOPPING_CHECKOUT_EXECUTE, SHOPPING_ORDER_READ};

/// Every Shopping operation requests the full lifecycle scope bundle at once.
/// This deliberately avoids disruptive OAuth consent/step-up during the common
/// catalog → checkout → order workflow.
pub(crate) const SHOPPING_SCOPES: &[&str] = &[
    SHOPPING_CATALOG_READ,
    SHOPPING_CHECKOUT_EXECUTE,
    SHOPPING_ORDER_READ,
];

pub fn module() -> Module {
    Module::new("Shopping", |ctx| {
        register_human_view(ctx);
        RuntimeGroupSpec::new(
            GroupSpec::new(
                "shopping",
                "Browse GoDaddy products, execute checkout, and view completed orders",
            )
            .with_long(
                "Browse GoDaddy products, create/update/complete checkout, and view completed orders.\n\
                 \n\
                 Shopping commands request the required OAuth permissions together so you can \
                 complete the catalog → checkout → order workflow without additional consent prompts.\n\
                 \n\
                 Use `gddy guide shopping` for request formats, checkout-completion safety, and \
                 environment configuration.",
            ),
        )
        .with_group(catalog::group())
        .with_group(checkout::group())
        .with_group(order::group())
    })
    .with_guides_from_markdown([(
        "shopping.md",
        include_bytes!("guides/shopping.md").as_slice(),
    )])
}
