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

/// Catalog browsing only reads the public product catalog, so it is a login
/// default and must not drag the purchase scopes into a step-up.
pub(crate) const SHOPPING_CATALOG_SCOPES: &[&str] = &[SHOPPING_CATALOG_READ];

/// Checkout spends money, so the checkout commands step up on first use; the
/// order read that follows a purchase rides along so a later `order get` is
/// already covered. `order get` itself declares only [`SHOPPING_ORDER_READ`],
/// so reading an order never asks for spending authority.
pub(crate) const SHOPPING_PURCHASE_SCOPES: &[&str] =
    &[SHOPPING_CHECKOUT_EXECUTE, SHOPPING_ORDER_READ];

pub(crate) const AGENT_PURCHASE_CONFIRMATION_INSTRUCTIONS: &str = "AI assistants: Before using --agree or completing checkout, show the end \
    user the checkout session's final total (including all fees), each \
    required agreement's title AND link, and every important link. Then \
    obtain their explicit confirmation to proceed. Do not summarize an \
    agreement by name only — the user must be able to click through and \
    read it. Do not infer approval from a request to purchase.";

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
