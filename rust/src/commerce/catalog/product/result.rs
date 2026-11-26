//! What the commands that change a product print: the product as it now is, and
//! the next step.

use cli_engine::{CommandResult, Result, TableColumn};
use serde_json::{Map, Value};

use crate::{
    commerce::catalog::follow_up, error::GddyError, next_action::required_value,
    output_schema::output_schema,
};

// What `create`, `update` and `archive` print: the product as it now is, or with
// `--dry-run` what would change.
output_schema!(ChangeResult {
    "id": "string", optional;
    "name": "string", optional;
    "label": "string", optional;
    "status": "string", optional;
    "type": "string", optional;
    "description": "string", optional;
    "variantCount": "number", optional;
    "priceMin": "number", optional;
    "priceMax": "number", optional;
    "compareAtPriceMin": "number", optional;
    "compareAtPriceMax": "number", optional;
    "htmlDescription": "string", optional;
    "createdAt": "string", optional;
    "updatedAt": "string", optional;
    "activatedAt": "string", optional;
    "archivedAt": "string", optional;
    "action": "string", optional;
    "productId": "string", optional;
    "changes": "object", optional;
    "clears": "[]string", optional;
});

/// The columns for a change a person reads. A change that was made is a product
/// and a preview is not, so each column is left out when the output has no value
/// for it: otherwise a preview would print a product's headings with nothing
/// after them, and a product would print the preview's.
///
/// The details a preview would send are columns of their own, reached by their
/// path in `changes`, and not a nested block: a block with no value in it (a
/// description that is only being cleared) is printed as a heading over blank
/// rows, where a column with no value is simply left out.
pub(super) fn change_columns() -> Vec<TableColumn> {
    let optional =
        |field: &str, header: &str| TableColumn::new(field, header).hide_when_empty(true);
    let mut columns = vec![
        optional("action", "Action"),
        optional("productId", "Product"),
        optional("clears", "Clears"),
        optional("changes.label", "New name"),
        optional("changes.type", "New type"),
        optional("changes.description", "New description"),
        optional("changes.status", "New status"),
        optional("changes.name", "New handle"),
        optional("changes.variants", "Variants"),
    ];
    columns.extend(
        super::get::view_columns()
            .into_iter()
            .map(|column| column.hide_when_empty(true)),
    );
    columns
}

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
    // A detail given as empty empties what is there. It is in `changes` as the
    // empty string it will be sent as, and the text view hides empty values, so it
    // is also listed here, where a person reading text will see it.
    let clears: Vec<Value> = details
        .iter()
        .filter(|(_, value)| value.is_some_and(str::is_empty))
        .map(|(name, _)| Value::from(*name))
        .collect();
    data.insert("changes".to_owned(), Value::Object(changes));
    if !clears.is_empty() {
        data.insert("clears".to_owned(), Value::Array(clears));
    }
    CommandResult::new(Value::Object(data)).with_dry_run()
}
