pub(super) mod complete;
mod create;
pub(super) mod get;
mod update;

use cli_engine::{GroupSpec, RuntimeGroupSpec};

pub(super) fn group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(
        GroupSpec::new("checkout", "Create and manage carts").with_long(
            "Create and update carts, then place an order after reviewing its payment methods and links. \
             Use `shopping order get` to review a completed purchase.",
        ),
    )
    .with_command(create::command())
    .with_command(get::command())
    .with_command(update::command())
    .with_command(complete::command())
}
