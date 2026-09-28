pub mod client;

mod catalog;
mod checkout;
mod common;
mod human;
mod money;
mod order;
mod product_actions;

use cli_engine::{GroupSpec, Module, RuntimeGroupSpec};

use crate::scopes::{SHOPPING_CATALOG_READ, SHOPPING_CHECKOUT_EXECUTE, SHOPPING_ORDER_READ};

pub(crate) const SHOPPING_SCOPES: &[&str] = &[
    SHOPPING_CATALOG_READ,
    SHOPPING_CHECKOUT_EXECUTE,
    SHOPPING_ORDER_READ,
];

pub(crate) const AGENT_AGREEMENT_CONFIRMATION_INSTRUCTIONS: &str = "AI assistants: Before using --agree or completing checkout, show all required agreements and important links to the end user and obtain their explicit confirmation. Do not infer agreement from a request to purchase.";

pub(crate) fn command_for_env(_env: &str, command: impl AsRef<str>) -> String {
    format!("shopping {}", command.as_ref())
}

pub fn module() -> Module {
    Module::new("Shopping", |ctx| {
        human::register_human_views(ctx);
        RuntimeGroupSpec::new(
            GroupSpec::new(
                "shopping",
                "Explore GoDaddy products, place orders, and review purchases",
            )
            .with_long(
                "Find GoDaddy products, add selected purchase options to a checkout session, place an \
                 order, and review your purchases. A checkout session is your cart. Use `gddy guide \
                 shopping` for a step-by-step purchase flow.",
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
