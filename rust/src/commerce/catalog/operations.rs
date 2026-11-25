//! The typed GraphQL operations the catalog client sends. Each is a
//! `graphql_client` query checked at compile time against the vendored schema
//! (`schemas/graphql/catalog-products.graphql`, refreshed by `cargo run -p
//! generate-api-catalog`), so a renamed or removed field fails the build here
//! instead of coming back as `null` at runtime.
//!
//! Exempt from the workspace's strict style lints: the file is derive-macro
//! output, as `commerce-client`'s generated module is.

#![allow(clippy::all)]
#![allow(dead_code)]
#![allow(missing_docs)]

use graphql_client::GraphQLQuery;

/// The catalog schema's `DateTime` scalar. `graphql_client` looks for a type of
/// the same name beside each query. Nothing here needs to parse it, so it stays
/// the string the API sent.
type DateTime = String;

/// One page of products (`skuGroups`).
#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "schemas/graphql/catalog-products.graphql",
    query_path = "src/commerce/catalog/graphql/product_list.graphql",
    response_derives = "Debug, Clone"
)]
pub(super) struct ProductList;
