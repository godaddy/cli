use cli_engine::{
    CommandResult, CommandSpec, NextAction, NextActionParam, Result, RuntimeCommandSpec, Tier,
};
use shopping_client::types::{
    CatalogSearchSearchRequest, CatalogSearchSearchResponse, SearchCatalogResponse, SearchRequest,
    Variant,
};

use crate::next_action::next_action;
use crate::output_schema::output_schema;
use crate::shopping::client::{ClientError, decode};
use crate::shopping::common::{
    client_err, currency_code, make_client, merge_context_currency, reject_response_errors,
};
use crate::shopping::human::{CATALOG_SEARCH_VIEW_ID, catalog_search_response};
use crate::shopping::{SHOPPING_SCOPES, command_for_env};

output_schema!(CatalogSearchOutput {
    "products": "[]object";
    "pagination": "object", optional;
    "messages": "[]object", optional;
});

#[derive(Debug, Clone, clap::Args)]
struct Args {
    /// Text to search for. Omit to browse the catalog.
    #[arg(long, value_name = "TEXT")]
    query: Option<String>,

    /// Product category to include. Run `shopping catalog categories` to list supported values. Repeat to include multiple categories.
    #[arg(long, value_name = "CATEGORY")]
    category: Vec<String>,

    /// Opaque cursor from the preceding catalog-search response.
    #[arg(long, value_name = "CURSOR")]
    cursor: Option<String>,

    /// Maximum number of products to return (1-100).
    #[arg(long, value_name = "N", value_parser = clap::value_parser!(u8).range(1..=100))]
    limit: Option<u8>,

    /// Preferred ISO 4217 currency for returned catalog prices (for example, USD or GBP).
    #[arg(long, value_name = "CODE", value_parser = currency_code)]
    currency: Option<String>,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<Args, _, _, _>(
        CommandSpec::from_args::<Args>("search", "Search the Shopping catalog")
            .with_long(
                "Explore GoDaddy products. Omit filters to browse the catalog. Use --query, repeatable \
                 --category, --cursor, --limit, and --currency to refine the results. Use \
                 --output json for the complete API response.",
            )
            .with_system("shopping")
            .with_tier(Tier::Read)
            .with_scopes(SHOPPING_SCOPES)
            .with_output_schema::<CatalogSearchOutput>()
            .with_view_id(CATALOG_SEARCH_VIEW_ID),
        |ctx, args: Args| async move {
            let mut request = CatalogSearchSearchRequest::default();
            merge_search_args(
                &mut request,
                args.query.as_deref(),
                &args.category,
                args.cursor.as_deref(),
            )?;
            merge_context_currency(&mut request.context, args.currency.as_deref());
            merge_pagination(&mut request, args.limit);
            let client = make_client(&ctx).await?;
            let response = decode::<SearchCatalogResponse>(
                client
                    .search_catalog()
                    .body(SearchRequest(request.clone()))
                    .send()
                    .await,
            )
            .await
            .map_err(client_err)?
            .ok_or_else(|| client_err(ClientError::EmptyResponse))?;
            let response = match response {
                SearchCatalogResponse::SearchResponse(response) => response.0,
                SearchCatalogResponse::ErrorResponse(payload) => {
                    return Err(client_err(
                        ClientError::UnexpectedErrorPayload(payload.into()),
                    ));
                }
            };
            reject_response_errors(&response.messages)?;
            let next_actions = next_actions(&response, &mut request, args.limit, &ctx.middleware.env)?;
            let response = serde_json::to_value(&response).map_err(|error| {
                crate::error::GddyError::unexpected(format!(
                    "failed to encode catalog search response: {error}"
                ))
                .into_cli_error()
            })?;
            let output = if ctx.middleware.output_format == "human" {
                catalog_search_response(&response)
            } else {
                response
            };
            Ok(CommandResult::new(output).with_next_actions(next_actions))
        },
    )
}

fn merge_search_args(
    request: &mut CatalogSearchSearchRequest,
    query: Option<&str>,
    categories: &[String],
    cursor: Option<&str>,
) -> Result<()> {
    if let Some(query) = query {
        request.query = Some(query.to_owned());
    }
    if !categories.is_empty() {
        let filters = request.filters.get_or_insert_with(Default::default);
        if !filters.categories.is_empty() {
            return Err(crate::error::GddyError::validation(
                "--category cannot be combined with an existing category filter",
            )
            .into_cli_error());
        }
        filters.categories = categories.to_vec();
    }
    if let Some(cursor) = cursor {
        request
            .pagination
            .get_or_insert_with(Default::default)
            .cursor = Some(cursor.to_owned());
    }
    Ok(())
}

fn merge_pagination(request: &mut CatalogSearchSearchRequest, limit: Option<u8>) {
    let Some(limit) = limit else {
        return;
    };
    let limit = std::num::NonZeroU64::new(u64::from(limit)).expect("clap enforces --limit >= 1");
    request
        .pagination
        .get_or_insert_with(Default::default)
        .limit = limit;
}

fn next_actions(
    response: &CatalogSearchSearchResponse,
    request: &mut CatalogSearchSearchRequest,
    requested_limit: Option<u8>,
    env: &str,
) -> Result<Vec<NextAction>> {
    let mut actions = product_actions(response, request, env);
    actions.extend(next_page_action(response, request, requested_limit, env)?);
    Ok(actions)
}

fn product_actions(
    response: &CatalogSearchSearchResponse,
    request: &CatalogSearchSearchRequest,
    env: &str,
) -> Vec<NextAction> {
    let Some(product) = response.products.first() else {
        return Vec::new();
    };
    let Some(product_id) = product.id.as_deref() else {
        return Vec::new();
    };
    let currency = request
        .context
        .as_ref()
        .and_then(|context| context.currency.as_deref());
    let mut get_action = next_action(
        command_for_env(env, "catalog get <product-id>"),
        "View the selected product's details",
    )
    .with_param("product-id", NextActionParam::value(product_id));
    if let Some(currency) = currency {
        get_action = get_action.with_param("currency", NextActionParam::value(currency));
        get_action.command.push_str(" --currency <currency>");
    }
    let mut actions = vec![get_action];
    if let Some(variant) = product
        .variants
        .iter()
        .find(|variant| is_available(variant))
    {
        let Some(variant_id) = variant.id.as_deref() else {
            return actions;
        };
        let currency = variant
            .price
            .as_ref()
            .and_then(|price| price.currency.as_deref())
            .unwrap_or("USD");
        actions.push(
            next_action(
                command_for_env(
                    env,
                    "checkout create --item <variant-id> --currency <currency>",
                ),
                "Add a purchase option to the checkout session.",
            )
            .with_param("variant-id", NextActionParam::value(variant_id))
            .with_param("currency", NextActionParam::value(currency)),
        );
    }
    actions
}

fn next_page_action(
    response: &CatalogSearchSearchResponse,
    request: &mut CatalogSearchSearchRequest,
    requested_limit: Option<u8>,
    env: &str,
) -> Result<Vec<NextAction>> {
    let Some(pagination) = response.pagination.as_ref() else {
        return Ok(Vec::new());
    };
    if !pagination.has_next_page.unwrap_or(false) {
        return Ok(Vec::new());
    }
    let cursor = pagination.cursor.clone().ok_or_else(|| {
        crate::error::GddyError::unexpected(
            "catalog search indicated another page but did not return a cursor",
        )
        .into_cli_error()
    })?;
    request
        .pagination
        .get_or_insert_with(Default::default)
        .cursor = Some(cursor);
    Ok(vec![search_action(request, requested_limit, env)?])
}

fn search_action(
    request: &CatalogSearchSearchRequest,
    requested_limit: Option<u8>,
    env: &str,
) -> Result<NextAction> {
    let mut command = "catalog search".to_owned();
    let mut params = Vec::new();
    append_search_param(&mut command, &mut params, "query", request.query.as_deref());
    if let Some(filters) = request.filters.as_ref() {
        for (index, category) in filters.categories.iter().enumerate() {
            let name = format!("category-{index}");
            command.push_str(&format!(" --category <{name}>"));
            params.push((name, category.clone()));
        }
    }
    append_search_param(
        &mut command,
        &mut params,
        "cursor",
        request
            .pagination
            .as_ref()
            .and_then(|pagination| pagination.cursor.as_deref()),
    );
    if let Some(limit) = requested_limit {
        command.push_str(&format!(" --limit {limit}"));
    }
    append_search_param(
        &mut command,
        &mut params,
        "currency",
        request
            .context
            .as_ref()
            .and_then(|context| context.currency.as_deref()),
    );
    Ok(params.into_iter().fold(
        next_action(command_for_env(env, command), "Fetch the next catalog page"),
        |action, (name, value)| action.with_param(name, NextActionParam::value(value)),
    ))
}

fn append_search_param(
    command: &mut String,
    params: &mut Vec<(String, String)>,
    name: &str,
    value: Option<&str>,
) {
    if let Some(value) = value {
        command.push_str(&format!(" --{name} <{name}>"));
        params.push((name.to_owned(), value.to_owned()));
    }
}

fn is_available(variant: &Variant) -> bool {
    variant
        .availability
        .as_ref()
        .and_then(|availability| availability.available)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use shopping_client::types::{
        Amount, Category, PaginationRequest, PaginationResponse, Price, Product, SearchFilters,
        VariantAvailability,
    };

    use super::{
        CatalogSearchSearchRequest, CatalogSearchSearchResponse, Variant, merge_pagination,
        merge_search_args, next_actions,
    };
    use crate::shopping::common::{currency_code, merge_context_currency};
    use crate::shopping::human::catalog_search_response;

    fn response() -> CatalogSearchSearchResponse {
        CatalogSearchSearchResponse {
            products: vec![Product {
                id: Some("product-1".to_owned()),
                title: Some("Product".to_owned()),
                categories: vec![Category {
                    value: Some("email".to_owned()),
                    ..Default::default()
                }],
                variants: vec![Variant {
                    id: Some("product-1:1yr".to_owned()),
                    title: Some("Product — 1 Year".to_owned()),
                    availability: Some(VariantAvailability {
                        available: Some(true),
                        ..Default::default()
                    }),
                    price: Some(Price {
                        amount: Some(Amount(7188)),
                        currency: Some("USD".to_owned()),
                    }),
                    list_price: Some(Price {
                        amount: Some(Amount(11988)),
                        currency: Some("USD".to_owned()),
                    }),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            pagination: Some(PaginationResponse {
                cursor: Some("next".to_owned()),
                has_next_page: Some(true),
                total_count: Some(14),
            }),
            messages: vec![],
            ucp: None,
        }
    }

    #[test]
    fn builds_search_request_without_a_json_body() {
        let mut request = CatalogSearchSearchRequest::default();
        merge_search_args(
            &mut request,
            Some("email"),
            &["email".to_owned(), "hosting".to_owned()],
            Some("cursor-1"),
        )
        .expect("flags should merge");
        merge_context_currency(&mut request.context, Some("GBP"));
        merge_pagination(&mut request, Some(3));

        assert_eq!(request.query, Some("email".to_owned()));
        assert_eq!(
            request.filters.expect("filters").categories,
            vec!["email".to_owned(), "hosting".to_owned()]
        );
        let pagination = request.pagination.expect("pagination");
        assert_eq!(pagination.cursor, Some("cursor-1".to_owned()));
        assert_eq!(pagination.limit.get(), 3);
        assert_eq!(
            request.context.expect("context").currency,
            Some("GBP".to_owned())
        );
    }

    #[test]
    fn merges_limit_with_the_requested_cursor() {
        let mut request = CatalogSearchSearchRequest {
            query: Some("email".to_owned()),
            pagination: Some(PaginationRequest {
                cursor: Some("cursor-1".to_owned()),
                ..Default::default()
            }),
            ..Default::default()
        };
        merge_pagination(&mut request, Some(25));
        let pagination = request.pagination.expect("pagination");
        assert_eq!(pagination.limit.get(), 25);
        assert_eq!(pagination.cursor, Some("cursor-1".to_owned()));
    }

    #[test]
    fn next_actions_keep_the_selected_environment() {
        let response = response();
        let mut request = CatalogSearchSearchRequest::default();
        let actions = next_actions(&response, &mut request, Some(3), "test").expect("actions");
        assert_eq!(actions.len(), 3);
        assert!(
            actions
                .iter()
                .all(|action| action.command.starts_with("gddy shopping"))
        );
        assert_eq!(actions[0].command, "gddy shopping catalog get <product-id>");
        assert_eq!(
            actions[0].params["product-id"].value.as_deref(),
            Some("product-1")
        );
        assert_eq!(
            actions[1].command,
            "gddy shopping checkout create --item <variant-id> --currency <currency>"
        );
        assert_eq!(
            actions[1].params["variant-id"].value.as_deref(),
            Some("product-1:1yr")
        );
        assert_eq!(actions[2].params["cursor"].value.as_deref(), Some("next"));
        assert_eq!(
            actions[2].command,
            "gddy shopping catalog search --cursor <cursor> --limit 3"
        );
    }

    #[test]
    fn next_actions_keep_dynamic_values_in_structured_parameters() {
        let response = response();
        let mut request = CatalogSearchSearchRequest {
            query: Some("O'Reilly".to_owned()),
            filters: Some(SearchFilters {
                categories: vec!["web & email".to_owned()],
                ..Default::default()
            }),
            ..Default::default()
        };
        let actions = next_actions(&response, &mut request, Some(3), "test").expect("actions");

        assert_eq!(
            actions[0].params["product-id"].value.as_deref(),
            Some("product-1")
        );
        assert_eq!(
            actions[2].params["query"].value.as_deref(),
            Some("O'Reilly")
        );
        assert_eq!(
            actions[2].params["category-0"].value.as_deref(),
            Some("web & email")
        );
        assert_eq!(actions[2].params["cursor"].value.as_deref(), Some("next"));
        assert!(!actions[2].command.contains("O'Reilly"));
        assert!(!actions[2].command.contains("web & email"));
    }

    #[test]
    fn human_output_retains_products_for_grouped_rendering() {
        let response = serde_json::to_value(response()).expect("serialize response");
        let response = catalog_search_response(&response);
        assert_eq!(response["products"].as_array().map(Vec::len), Some(1));
        assert_eq!(response["products"][0]["title"], "Product");
        assert_eq!(
            response["products"][0]["variants"][0]["id"],
            "product-1:1yr"
        );
        assert_eq!(response["products"][0]["variants"][0]["price"], "USD 71.88");
    }

    #[test]
    fn merges_currency_and_preserves_it_for_follow_up_actions() {
        let response = response();
        let mut request = CatalogSearchSearchRequest::default();
        merge_context_currency(&mut request.context, Some("jpy"));
        let actions = next_actions(&response, &mut request, None, "test").expect("actions");

        assert_eq!(
            request
                .context
                .as_ref()
                .and_then(|context| context.currency.clone()),
            Some("jpy".to_owned())
        );
        assert_eq!(actions[1].params["currency"].value.as_deref(), Some("USD"));
        assert_eq!(actions[2].params["currency"].value.as_deref(), Some("jpy"));
    }

    #[test]
    fn validates_and_normalizes_currency_codes() {
        assert_eq!(currency_code(" gbp ").expect("valid currency"), "GBP");
        assert_eq!(currency_code("jpy").expect("valid currency"), "JPY");
        assert!(currency_code("ZZZ").is_err());
        assert!(currency_code("JP").is_err());
        assert!(currency_code("123").is_err());
    }
}
