use std::collections::BTreeSet;

use cli_engine::NextAction;
use shopping_client::types::{Checkout, LookupResponse};

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
pub(crate) fn purchased_product_ids(checkout: &Checkout) -> Vec<String> {
    checkout
        .line_items
        .iter()
        .filter_map(|line_item| line_item.item.as_ref()?.id.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// Returns post-purchase actions for categories in a Shopping catalog lookup response.
pub(crate) fn post_purchase_actions(catalog_response: &LookupResponse) -> Vec<NextAction> {
    let categories = catalog_response
        .0
        .products
        .iter()
        .flat_map(|product| product.categories.iter())
        .filter_map(|category| category.value.as_deref())
        .collect::<BTreeSet<_>>();

    CATEGORY_ACTIONS
        .iter()
        .filter(|(category, _, _)| categories.contains(category))
        .map(|(_, command, description)| next_action(*command, *description))
        .collect()
}

#[cfg(test)]
mod tests {
    use shopping_client::types::{
        CatalogLookupLookupResponse, CatalogLookupLookupResponseProductsItem, Category, Checkout,
        Item, LineItem, LookupResponse,
    };

    use super::{post_purchase_actions, purchased_product_ids};

    fn line_item(id: Option<&str>) -> LineItem {
        LineItem {
            item: Some(Item {
                id: id.map(str::to_owned),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn product_with_categories(categories: &[&str]) -> CatalogLookupLookupResponseProductsItem {
        CatalogLookupLookupResponseProductsItem {
            categories: categories
                .iter()
                .map(|value| Category {
                    value: Some((*value).to_owned()),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn returns_unique_product_ids_from_checkout_items() {
        let checkout = Checkout {
            line_items: vec![
                line_item(Some("email-product")),
                line_item(Some("hosting-product")),
                line_item(Some("email-product")),
                line_item(None),
            ],
            ..Default::default()
        };

        assert_eq!(
            purchased_product_ids(&checkout),
            ["email-product", "hosting-product"]
        );
    }

    #[test]
    fn post_purchase_actions_follow_catalog_categories() {
        let lookup = LookupResponse(CatalogLookupLookupResponse {
            products: vec![
                product_with_categories(&["email"]),
                product_with_categories(&["webHosting"]),
                product_with_categories(&["email"]),
                product_with_categories(&["domains"]),
            ],
            ..Default::default()
        });

        let actions = post_purchase_actions(&lookup);

        assert_eq!(actions.len(), 2);
        assert_eq!(actions[0].command, "gddy guide hosting");
        assert_eq!(actions[1].command, "gddy guide email");
    }

    #[test]
    fn post_purchase_actions_ignore_unknown_or_missing_categories() {
        let lookup = LookupResponse(CatalogLookupLookupResponse {
            products: vec![
                product_with_categories(&["domains"]),
                CatalogLookupLookupResponseProductsItem::default(),
            ],
            ..Default::default()
        });

        assert!(post_purchase_actions(&lookup).is_empty());
    }
}
