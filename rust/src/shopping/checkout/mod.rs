pub(super) mod complete;
mod create;
pub(super) mod get;
mod update;

use cli_engine::{GroupSpec, RuntimeGroupSpec};

pub(super) fn group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(
        GroupSpec::new("checkout", "Create and manage checkout sessions").with_long(
            "A checkout session is your cart. Create or update one, then review its currently selected and \
             eligible saved payment methods, required agreements, and important links before placing an order. Checkout responses \
             show the first five payment methods by default; use --show-all-payment-instruments for all. \
             Use `shopping order get` to review a completed purchase.",
        ),
    )
    .with_command(create::command())
    .with_command(get::command())
    .with_command(update::command())
    .with_command(complete::command())
}
