pub(super) mod categories;
pub(super) mod get;
pub(super) mod lookup;
pub(super) mod search;

use cli_engine::{GroupSpec, NextAction, NextActionParam, RuntimeGroupSpec};

use crate::next_action::next_action;
use crate::shopping::command_for_env;

/// Suggests starting a checkout session with a purchase option the catalog
/// reported as available; shared by every catalog command that shows one.
pub(super) fn checkout_create_action(env: &str, variant_id: &str, currency: &str) -> NextAction {
    next_action(
        command_for_env(
            env,
            "checkout create --item <variant-id> --currency <currency>",
        ),
        "Add a purchase option to the checkout session.",
    )
    .with_param("variant-id", NextActionParam::value(variant_id))
    .with_param("currency", NextActionParam::value(currency))
}

pub(super) fn group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(
        GroupSpec::new("catalog", "Explore GoDaddy's product catalog")
            .with_alias("catalogs")
            .with_long("Search GoDaddy's product catalog and retrieve product details."),
    )
    .with_command(categories::command())
    .with_command(search::command())
    .with_command(lookup::command())
    .with_command(get::command())
}
