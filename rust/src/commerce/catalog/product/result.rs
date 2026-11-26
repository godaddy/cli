//! What the commands that change a product print: the product as it now is, and
//! the next step.

use cli_engine::{CommandResult, Result};
use serde_json::{Map, Value};

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

/// What a `--dry-run` of a change prints: the action, and the details that would
/// be sent, only those the person gave. Marked as a preview so a consumer can
/// tell it from a change that was made.
pub(super) fn preview(
    action: &str,
    product_id: Option<&str>,
    details: &[(&str, Option<&str>)],
) -> CommandResult {
    let changes: Map<String, Value> = details
        .iter()
        .filter_map(|(name, value)| value.map(|value| ((*name).to_owned(), Value::from(value))))
        .collect();
    let mut data = Map::new();
    data.insert("action".to_owned(), Value::from(action));
    if let Some(id) = product_id {
        data.insert("productId".to_owned(), Value::from(id));
    }
    data.insert("changes".to_owned(), Value::Object(changes));
    CommandResult::new(Value::Object(data)).with_dry_run()
}
