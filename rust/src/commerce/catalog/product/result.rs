//! What the commands that change a product print: the product as it now is, and
//! the next step.

use cli_engine::{CommandResult, Result};
use serde_json::Value;

use crate::{commerce::catalog::follow_up, error::GddyError, next_action::required_value};

/// The result of a change: `product` as the API now has it, and a step to read it
/// back. A product that cannot be turned into output is an error, not an empty
/// result.
pub(super) fn changed(
    product: std::result::Result<Value, serde_json::Error>,
    store_flag: Option<&str>,
) -> Result<CommandResult> {
    let data = product.map_err(|_| {
        GddyError::unexpected("the product could not be shown")
            .with_system("commerce")
            .into_cli_error()
    })?;
    let id = data["id"].as_str().map(str::to_owned);
    let result = CommandResult::new(data);
    Ok(match id {
        Some(id) => result.with_next_actions(vec![
            follow_up(
                "commerce catalog product get <product-id>",
                "Read the product back",
                store_flag,
            )
            .with_param("product-id", required_value(id)),
        ]),
        None => result,
    })
}
