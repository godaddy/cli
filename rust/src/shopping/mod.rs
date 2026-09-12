pub mod client;

mod catalog;
mod checkout;
mod common;
mod money;
mod order;

use cli_engine::{GroupSpec, Module, RuntimeGroupSpec};

use crate::shopping::catalog::get::register_human_view as register_catalog_get_human_view;
use crate::shopping::catalog::lookup::register_human_view as register_catalog_lookup_human_view;
use crate::shopping::catalog::search::register_human_view as register_catalog_search_human_view;
use crate::shopping::checkout::complete::register_human_view as register_checkout_complete_human_view;
use crate::shopping::checkout::get::register_human_view as register_checkout_get_human_view;
use crate::shopping::order::get::register_human_view as register_order_get_human_view;

use crate::scopes::{SHOPPING_CATALOG_READ, SHOPPING_CHECKOUT_EXECUTE, SHOPPING_ORDER_READ};

pub(crate) const SHOPPING_SCOPES: &[&str] = &[
    SHOPPING_CATALOG_READ,
    SHOPPING_CHECKOUT_EXECUTE,
    SHOPPING_ORDER_READ,
];

pub(crate) fn command_for_env(_env: &str, command: impl AsRef<str>) -> String {
    format!("shopping {}", command.as_ref())
}

pub fn module() -> Module {
    Module::new("Shopping", |ctx| {
        register_catalog_get_human_view(ctx);
        register_catalog_lookup_human_view(ctx);
        register_catalog_search_human_view(ctx);
        register_checkout_complete_human_view(ctx);
        register_checkout_get_human_view(ctx);
        register_order_get_human_view(ctx);
        RuntimeGroupSpec::new(
            GroupSpec::new(
                "shopping",
                "Explore GoDaddy products, place orders, and review purchases",
            )
            .with_long(
                "Find GoDaddy products, add them to a cart, place an order, and review your purchases. \
                 Use `gddy guide shopping` for a step-by-step purchase flow.",
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
