pub mod client;

mod catalog;
mod checkout;
mod common;
mod order;

use cli_engine::{GroupSpec, Module, RuntimeGroupSpec};

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
    Module::new("Shopping", |_ctx| {
        RuntimeGroupSpec::new(
            GroupSpec::new("shopping", "Browse catalog products and complete purchases").with_long(
                "Use the direct Order Management Shopping API integration. Every \
                 command requests catalog, checkout, and order OAuth scopes together, allowing a \
                 single consent flow for the catalog → checkout → order lifecycle.\n\
                 \n\
                 The direct Katana endpoint must be configured with shopping_url in \
                 ~/.config/gddy/environments.toml (or SHOPPING_URL). PATs do not work against \
                 this direct service until front-door token exchange is available.\n\
                 \n\
                 checkout complete places a real order and must include an idempotency_key. Follow \
                 completion with `shopping order get <order-id> --wait`; checkout get is not valid \
                 for completed sessions.",
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
