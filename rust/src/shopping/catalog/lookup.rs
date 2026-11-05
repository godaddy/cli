use cli_engine::{CommandResult, CommandSpec, RuntimeCommandSpec, Tier};
use shopping_client::types::{
    CatalogLookupLookupRequest, CatalogLookupLookupResponse, LookupCatalogResponse, LookupRequest,
};

use crate::output_schema::output_schema;
use crate::shopping::SHOPPING_CATALOG_SCOPES;
use crate::shopping::client::{ClientError, decode};
use crate::shopping::common::{
    client_err, make_client, merge_context_currency, reject_response_errors,
};
use crate::shopping::currency::CurrencyArgs;
use crate::shopping::human::{CATALOG_LOOKUP_VIEW_ID, catalog_lookup_response};

output_schema!(CatalogLookupOutput {
    "ucp": "object";
    "products": "[]object";
    "messages": "[]object";
});

#[derive(Debug, Clone, clap::Args)]
struct Args {
    /// Product or purchase option ID to resolve. Repeat to resolve multiple IDs.
    #[arg(value_name = "ID", required = true)]
    id: Vec<String>,

    #[command(flatten)]
    currency: CurrencyArgs,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<Args, _, _, _>(
        CommandSpec::from_args::<Args>("lookup", "Find products by ID")
            .with_long(
                "Find one or more products or purchase options by ID. Unknown IDs are reported in the \
                 response without preventing matches for the other IDs.",
            )
            .with_system("shopping")
            .with_tier(Tier::Read)
            .with_scopes(SHOPPING_CATALOG_SCOPES)
            .with_output_schema::<CatalogLookupOutput>()
            .with_view_id(CATALOG_LOOKUP_VIEW_ID),
        |ctx, args: Args| async move {
            let mut request = CatalogLookupLookupRequest {
                ids: args.id,
                ..Default::default()
            };
            let currency = args.currency.resolve(&ctx)?;
            merge_context_currency(&mut request.context, currency.as_deref());
            let client = make_client(&ctx).await?;
            let response = decode::<LookupCatalogResponse>(
                client
                    .lookup_catalog()
                    .body(LookupRequest(request))
                    .send()
                    .await,
            )
            .await
            .map_err(client_err)?
            .ok_or_else(|| client_err(ClientError::EmptyResponse))?;
            let response = match response {
                LookupCatalogResponse::LookupResponse(response) => response.0,
                LookupCatalogResponse::ErrorResponse(payload) => {
                    return Err(client_err(ClientError::UnexpectedErrorPayload(
                        payload.into(),
                    )));
                }
            };
            reject_response_errors(&response.messages)?;
            let actions = next_actions(&response, &ctx.middleware.env);
            let response = serde_json::to_value(&response).map_err(|error| {
                crate::error::GddyError::unexpected(format!(
                    "failed to encode catalog lookup response: {error}"
                ))
                .into_cli_error()
            })?;
            let output = if ctx.middleware.output_format == "human" {
                catalog_lookup_response(&response)
            } else {
                response
            };
            Ok(CommandResult::new(output).with_next_actions(actions))
        },
    )
}

/// Suggests a checkout for the first purchase option the lookup reports as available.
fn next_actions(response: &CatalogLookupLookupResponse, env: &str) -> Vec<cli_engine::NextAction> {
    response
        .products
        .iter()
        .flat_map(|product| product.variants.iter())
        .find_map(|variant| {
            let available = variant
                .availability
                .as_ref()
                .and_then(|availability| availability.available)
                .unwrap_or(false);
            let id = variant.id.as_deref()?;
            available.then(|| {
                let currency = variant
                    .price
                    .as_ref()
                    .and_then(|price| price.currency.as_deref())
                    .unwrap_or("USD");
                super::checkout_create_action(env, id, currency)
            })
        })
        .into_iter()
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use shopping_client::types::{
        CatalogLookupLookupResponseProductsItem,
        CatalogLookupLookupResponseProductsItemVariantsItem,
        CatalogLookupLookupResponseProductsItemVariantsItemAvailability, Price,
    };

    use super::*;

    fn variant(
        id: &str,
        available: bool,
        currency: &str,
    ) -> CatalogLookupLookupResponseProductsItemVariantsItem {
        CatalogLookupLookupResponseProductsItemVariantsItem {
            id: Some(id.to_owned()),
            availability: Some(
                CatalogLookupLookupResponseProductsItemVariantsItemAvailability {
                    available: Some(available),
                    ..Default::default()
                },
            ),
            price: Some(Price {
                currency: Some(currency.to_owned()),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn response(
        variants: Vec<CatalogLookupLookupResponseProductsItemVariantsItem>,
    ) -> CatalogLookupLookupResponse {
        CatalogLookupLookupResponse {
            products: vec![CatalogLookupLookupResponseProductsItem {
                variants,
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    #[test]
    fn suggests_checkout_for_the_first_available_purchase_option() {
        let actions = next_actions(
            &response(vec![
                variant("sold-out", false, "USD"),
                variant("product-1:1yr", true, "GBP"),
            ]),
            "test",
        );

        assert_eq!(actions.len(), 1);
        assert!(actions[0].command.contains("checkout create"));
        assert_eq!(
            actions[0].params["variant-id"].value.as_deref(),
            Some("product-1:1yr")
        );
        assert_eq!(actions[0].params["currency"].value.as_deref(), Some("GBP"));
    }

    #[test]
    fn suggests_nothing_when_no_purchase_option_is_available() {
        assert!(
            next_actions(&response(vec![variant("sold-out", false, "USD")]), "test").is_empty()
        );
        assert!(next_actions(&CatalogLookupLookupResponse::default(), "test").is_empty());
    }

    #[test]
    fn human_response_shows_lookup_essentials_without_ucp_metadata() {
        let output = catalog_lookup_response(&json!({
            "products": [{
                "id": "product-1",
                "title": "Product",
                "categories": [{"value": "email"}],
                "price_range": {
                    "min": {"amount": 7188, "currency": "USD"},
                    "max": {"amount": 7188, "currency": "USD"}
                },
                "variants": [{
                    "id": "product-1:1yr",
                    "title": "One year",
                    "availability": {"available": true},
                    "price": {"amount": 7188, "currency": "USD"},
                    "list_price": {"amount": 11988, "currency": "USD"}
                }]
            }],
            "ucp": {"do_not_render": true}
        }));

        assert_eq!(output["products"][0]["title"], "Product");
        assert_eq!(output["products"][0]["variants"][0]["price"], "USD 71.88");
        assert!(!output.to_string().contains("do_not_render"));
    }
}
