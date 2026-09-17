use cli_engine::{CommandResult, CommandSpec, NextActionParam, RuntimeCommandSpec, Tier};
use serde_json::{Value, json};

use crate::next_action::next_action;
use crate::output_schema::output_schema;
use crate::shopping::common::{client_err, currency_code, make_client, merge_context_currency};
use crate::shopping::human::{CATALOG_GET_VIEW_ID, catalog_product_response};
use crate::shopping::{SHOPPING_SCOPES, command_for_env};

output_schema!(CatalogProductOutput {
    "ucp": "object";
    "product": "object";
});

#[derive(Debug, Clone, clap::Args)]
struct Args {
    /// Product or purchase option ID to retrieve.
    #[arg(value_name = "ID")]
    id: String,

    /// Preferred ISO 4217 currency for returned catalog prices (for example, USD or GBP).
    #[arg(long, value_name = "CODE", value_parser = currency_code)]
    currency: Option<String>,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<Args, _, _, _>(
        CommandSpec::from_args::<Args>("get", "View product details")
            .with_long("View one product or purchase option by ID, including available options and prices.")
            .with_system("shopping")
            .with_tier(Tier::Read)
            .with_scopes(SHOPPING_SCOPES)
            .with_output_schema::<CatalogProductOutput>()
            .with_view_id(CATALOG_GET_VIEW_ID),
        |ctx, args: Args| async move {
            let mut body = json!({"id": args.id});
            merge_context_currency(&mut body, args.currency.as_deref())?;
            let client = make_client(&ctx).await?;
            let response = client.catalog_product(body).await.map_err(client_err)?;
            let actions = next_actions(&response, &ctx.middleware.env);
            let output = if ctx.middleware.output_format == "human" {
                catalog_product_response(&response)
            } else {
                response
            };
            Ok(CommandResult::new(output).with_next_actions(actions))
        },
    )
}

fn next_actions(response: &Value, env: &str) -> Vec<cli_engine::NextAction> {
    let Some(product) = response.get("product") else {
        return Vec::new();
    };
    let Some((variant_id, currency)) =
        product
            .get("variants")
            .and_then(Value::as_array)
            .and_then(|variants| {
                variants.iter().find_map(|variant| {
                    let available = variant
                        .pointer("/availability/available")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    let id = variant.get("id").and_then(Value::as_str)?;
                    available.then(|| {
                        (
                            id,
                            variant
                                .pointer("/price/currency")
                                .and_then(Value::as_str)
                                .unwrap_or("USD"),
                        )
                    })
                })
            })
    else {
        return Vec::new();
    };
    vec![
        next_action(
            command_for_env(
                env,
                "checkout create --item <variant-id> --currency <currency>",
            ),
            "Add a purchase option to the checkout session.",
        )
        .with_param("variant-id", NextActionParam::value(variant_id))
        .with_param("currency", NextActionParam::value(currency)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shopping::human::catalog_product_response;

    #[test]
    fn product_response_shows_essentials_and_checkout_action() {
        let response = json!({
            "product": {
                "id": "product-1",
                "title": "Product",
                "description": {"plain": "Description"},
                "categories": [{"value": "email"}],
                "price_range": {"min": {"amount": 7188, "currency": "USD"}, "max": {"amount": 7188, "currency": "USD"}},
                "variants": [{"id": "variant-1", "title": "One year", "availability": {"available": true}, "price": {"amount": 7188, "currency": "USD"}, "list_price": {"amount": 11988, "currency": "USD"}}]
            },
            "ucp": {"do_not_render": true}
        });
        let output = catalog_product_response(&response);
        let actions = next_actions(&response, "test");

        assert_eq!(output["title"], "Product");
        assert_eq!(output["variants"][0]["price"], "USD 71.88");
        assert_eq!(
            actions[0].description,
            "Add a purchase option to the checkout session."
        );
        assert!(actions[0].command.contains("checkout create"));
        assert!(!output.to_string().contains("do_not_render"));
    }
}
