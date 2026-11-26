//! What the product commands print. The API calls a product a `SKUGroup` and
//! names its fields accordingly; these types use the words a merchant uses, and
//! carry only what a command shows, so the JSON a script reads does not change
//! when the API's internal shape does.
//!
//! A price range is two plain numbers, not an object, so a table can show each
//! as a column and a script reads them as numbers. They are in the store's
//! currency, which the API does not state alongside them.

use serde::Serialize;

use crate::commerce::catalog::operations::{
    product, product_archive, product_create, product_list, product_update,
};

/// One product as `product list` shows it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ProductSummary {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) label: String,
    pub(super) status: String,
    #[serde(rename = "type")]
    pub(super) kind: String,
    pub(super) description: Option<String>,
    pub(super) variant_count: Option<i64>,
    pub(super) price_min: Option<f64>,
    pub(super) price_max: Option<f64>,
    pub(super) compare_at_price_min: Option<f64>,
    pub(super) compare_at_price_max: Option<f64>,
    pub(super) created_at: String,
    pub(super) updated_at: String,
}

/// One product as `product get` shows it: the summary plus what only a single
/// product's page carries.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ProductDetail {
    #[serde(flatten)]
    pub(super) summary: ProductSummary,
    pub(super) html_description: Option<String>,
    pub(super) activated_at: Option<String>,
    pub(super) archived_at: Option<String>,
}

/// The two ends of a price range, or none when the product has no priced
/// variant.
fn bounds(range: Option<(Option<f64>, Option<f64>)>) -> (Option<f64>, Option<f64>) {
    range.unwrap_or((None, None))
}

impl From<product_list::ProductListSkuGroupsEdgesNode> for ProductSummary {
    fn from(node: product_list::ProductListSkuGroupsEdgesNode) -> Self {
        let (price_min, price_max) = bounds(node.price_range.map(|range| (range.min, range.max)));
        let (compare_at_price_min, compare_at_price_max) = bounds(
            node.compare_at_price_range
                .map(|range| (range.min, range.max)),
        );
        Self {
            id: node.id,
            name: node.name,
            label: node.label,
            status: node.status,
            kind: node.type_,
            description: node.description,
            variant_count: node.sku_count,
            price_min,
            price_max,
            compare_at_price_min,
            compare_at_price_max,
            created_at: node.created_at,
            updated_at: node.updated_at,
        }
    }
}

/// `ProductDetail` from the product each operation answers with. The operations
/// each have a type of their own for the same selection, with the same fields.
macro_rules! detail_from {
    ($($node:ty),+ $(,)?) => {$(
        impl From<$node> for ProductDetail {
            fn from(node: $node) -> Self {
                let (price_min, price_max) =
                    bounds(node.price_range.map(|range| (range.min, range.max)));
                let (compare_at_price_min, compare_at_price_max) = bounds(
                    node.compare_at_price_range
                        .map(|range| (range.min, range.max)),
                );
                Self {
                    html_description: node.html_description,
                    activated_at: node.activated_at,
                    archived_at: node.archived_at,
                    summary: ProductSummary {
                        id: node.id,
                        name: node.name,
                        label: node.label,
                        status: node.status,
                        kind: node.type_,
                        description: node.description,
                        variant_count: node.sku_count,
                        price_min,
                        price_max,
                        compare_at_price_min,
                        compare_at_price_max,
                        created_at: node.created_at,
                        updated_at: node.updated_at,
                    },
                }
            }
        }
    )+};
}

detail_from!(
    product::ProductSkuGroup,
    product_create::ProductCreateCreateSkuGroup,
    product_update::ProductUpdateUpdateSkuGroup,
    product_archive::ProductArchiveArchiveSkuGroup,
);
