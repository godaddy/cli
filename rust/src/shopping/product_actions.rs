use std::collections::BTreeSet;

use cli_engine::NextAction;
use serde_json::Value;

use crate::next_action::next_action;

const CATEGORY_ACTIONS: &[(&str, &str, &str)] = &[
    (
        "webHosting",
        "guide hosting",
        "Set up your Web Hosting product",
    ),
    ("email", "guide email", "Set up your Email product"),
];

/// Returns unique purchasable product IDs from a checkout session.
pub(crate) fn purchased_product_ids(checkout: &Value) -> Vec<String> {
    checkout
        .get("line_items")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|line_item| line_item.pointer("/item/id").and_then(Value::as_str))
        .map(str::to_owned)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// Returns post-purchase actions for categories in a Shopping catalog lookup response.
pub(crate) fn post_purchase_actions(catalog_response: &Value) -> Vec<NextAction> {
    let categories = catalog_response
        .get("products")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(|product| {
            product
                .get("categories")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
        })
        .filter_map(|category| category.get("value").and_then(Value::as_str))
        .collect::<BTreeSet<_>>();

    CATEGORY_ACTIONS
        .iter()
        .filter(|(category, _, _)| categories.contains(category))
        .map(|(_, command, description)| next_action(*command, *description))
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{post_purchase_actions, purchased_product_ids};

    #[test]
    fn returns_unique_product_ids_from_checkout_items() {
        let checkout = json!({
            "line_items": [
                {"item": {"id": "email-product"}},
                {"item": {"id": "hosting-product"}},
                {"item": {"id": "email-product"}},
                {"item": {}}
            ]
        });

        assert_eq!(
            purchased_product_ids(&checkout),
            ["email-product", "hosting-product"]
        );
    }

    #[test]
    fn post_purchase_actions_follow_catalog_categories() {
        let lookup = json!({
            "products": [
                {"categories": [{"value": "email"}]},
                {"categories": [{"value": "webHosting"}]},
                {"categories": [{"value": "email"}]},
                {"categories": [{"value": "domains"}]}
            ]
        });

        let actions = post_purchase_actions(&lookup);

        assert_eq!(actions.len(), 2);
        assert_eq!(actions[0].command, "gddy guide hosting");
        assert_eq!(actions[1].command, "gddy guide email");
    }

    #[test]
    fn post_purchase_actions_ignore_unknown_or_missing_categories() {
        let lookup = json!({
            "products": [
                {"categories": [{"value": "domains"}]},
                {}
            ]
        });

        assert!(post_purchase_actions(&lookup).is_empty());
    }
}
