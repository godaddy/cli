use cli_engine::{CommandResult, CommandSpec, RuntimeCommandSpec, Tier};
use shopping_client::types::{
    CatalogLookupLookupRequest, LookupCatalogResponse, LookupRequest, LookupResponse,
};

use crate::output_schema::output_schema;
use crate::shopping::SHOPPING_SCOPES;
use crate::shopping::client::{ClientError, decode};
use crate::shopping::common::{client_err, currency_code, make_client, merge_context_currency};
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

    /// Preferred ISO 4217 currency for returned catalog prices (for example, USD or GBP).
    #[arg(long, value_name = "CODE", value_parser = currency_code)]
    currency: Option<String>,
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
            .with_scopes(SHOPPING_SCOPES)
            .with_output_schema::<CatalogLookupOutput>()
            .with_view_id(CATALOG_LOOKUP_VIEW_ID),
        |ctx, args: Args| async move {
            let mut request = CatalogLookupLookupRequest {
                ids: args.id,
                ..Default::default()
            };
            merge_context_currency(&mut request.context, args.currency.as_deref());
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
            .unwrap_or_else(|| {
                LookupCatalogResponse::LookupResponse(LookupResponse(Default::default()))
            });
            let response = match response {
                LookupCatalogResponse::LookupResponse(response) => response.0,
                LookupCatalogResponse::ErrorResponse(payload) => {
                    return Err(client_err(ClientError::UnexpectedErrorPayload(
                        payload.into(),
                    )));
                }
            };
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
            Ok(CommandResult::new(output))
        },
    )
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

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
