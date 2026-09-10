use cli_engine::{CommandResult, CommandSpec, ModuleContext, RuntimeCommandSpec, Tier};
use serde_json::{Value, json};

use crate::next_action::{next_action, required_value};
use crate::output_schema::output_schema;
use crate::shopping::common::{
    client_err, currency_code, make_client, merge_context_currency, read_json,
};
use crate::shopping::money;
use crate::shopping::{SHOPPING_SCOPES, command_for_env};

output_schema!(CatalogProductOutput {
    "ucp": "object";
    "product": "object";
});

#[derive(Debug, Clone, clap::Args)]
struct Args {
    /// Product or variant ID to retrieve.
    #[arg(long, value_name = "ID", required_unless_present_any = ["body", "file"])]
    id: Option<String>,

    /// Preferred ISO 4217 currency for returned catalog prices (for example, USD or GBP).
    #[arg(long, value_name = "CODE", value_parser = currency_code)]
    currency: Option<String>,

    /// Product request as raw JSON for advanced Shopping API selections and preferences.
    #[arg(long, value_name = "JSON")]
    body: Option<String>,

    /// Path to a JSON product request. Takes precedence over --body.
    #[arg(long, value_name = "PATH")]
    file: Option<String>,
}

const HUMAN_VIEW_ID: &str = "shopping-catalog-get";

pub(crate) fn register_human_view(ctx: &mut ModuleContext<'_>) {
    ctx.middleware_mut()
        .human_views
        .register_func(HUMAN_VIEW_ID, render_human);
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<Args, _, _, _>(
        CommandSpec::from_args::<Args>("get", "Get one Shopping catalog product")
            .with_long(
                "Get one product or variant with --id. Use --body or --file only for advanced \
                 Shopping API selections and preferences.",
            )
            .with_system("shopping")
            .with_tier(Tier::Read)
            .with_scopes(SHOPPING_SCOPES)
            .with_output_schema::<CatalogProductOutput>()
            .with_view_id(HUMAN_VIEW_ID),
        |ctx, args: Args| async move {
            let mut body = if args.body.is_some() || args.file.is_some() {
                read_json(args.body.as_deref(), args.file.as_deref(), "object")?
            } else {
                json!({})
            };
            if let Some(id) = args.id {
                let object = body
                    .as_object_mut()
                    .expect("catalog product request is an object");
                if object.contains_key("id") {
                    return Err(crate::error::GddyError::validation(
                        "--id conflicts with id in the request body",
                    )
                    .into_cli_error());
                }
                object.insert("id".to_owned(), json!(id));
            }
            merge_context_currency(&mut body, args.currency.as_deref())?;
            let client = make_client(&ctx).await?;
            let response = client.catalog_product(body).await.map_err(client_err)?;
            let actions = next_actions(&response, &ctx.middleware.env);
            let output = if ctx.middleware.output_format == "human" {
                human_response(&response, &actions)
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
                format!("checkout create --item '{variant_id}' --currency {currency}"),
            ),
            "Create a checkout with the first available variant",
        )
        .with_param("variant_id", required_value(variant_id)),
    ]
}

fn human_response(response: &Value, actions: &[cli_engine::NextAction]) -> Value {
    let product = response.get("product").cloned().unwrap_or(Value::Null);
    json!({
        "id": product.get("id").and_then(Value::as_str).unwrap_or_default(),
        "title": product.get("title").and_then(Value::as_str).unwrap_or("Untitled product"),
        "description": product.pointer("/description/plain").and_then(Value::as_str),
        "categories": product.get("categories").and_then(Value::as_array).map(|categories| categories.iter().filter_map(|category| category.get("value").and_then(Value::as_str)).collect::<Vec<_>>()).unwrap_or_default(),
        "price_range": product.get("price_range").cloned(),
        "variants": product.get("variants").cloned().unwrap_or_else(|| json!([])),
        "next_steps": actions.iter().map(|action| json!({"command": action.command, "description": action.description})).collect::<Vec<_>>(),
    })
}

fn render_human(product: &Value) -> String {
    let mut output = format!(
        "{} (ID: {})\n",
        product
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("Untitled product"),
        product
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    );
    if let Some(description) = product.get("description").and_then(Value::as_str) {
        output.push_str(&format!("{description}\n"));
    }
    let categories = product
        .get("categories")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    if !categories.is_empty() {
        output.push_str(&format!(
            "Category: {}\n",
            categories
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if let Some(range) = product.get("price_range") {
        let min = money(range.get("min"));
        let max = money(range.get("max"));
        if let Some(price_range) = match (min, max) {
            (Some(min), Some(max)) if min == max => Some(min),
            (Some(min), Some(max)) => Some(format!("{min}–{max}")),
            _ => None,
        } {
            output.push_str(&format!("Price range: {price_range}\n"));
        }
    }
    output.push_str("\nVariants:\n");
    let variants = product
        .get("variants")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    if variants.is_empty() {
        output.push_str("- None\n");
    }
    for variant in variants {
        let availability = if variant
            .pointer("/availability/available")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            "Available"
        } else {
            "Unavailable"
        };
        output.push_str(&format!(
            "- {} (ID: {})\n  Price: {} · List price: {} · {availability}\n",
            variant
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or("Untitled variant"),
            variant
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            money(variant.get("price")).unwrap_or_else(|| "Unavailable".to_owned()),
            money(variant.get("list_price")).unwrap_or_else(|| "Unavailable".to_owned()),
        ));
    }
    render_next_steps(&mut output, product);
    output
}

fn render_next_steps(output: &mut String, response: &Value) {
    let steps = response
        .get("next_steps")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    if steps.is_empty() {
        return;
    }
    output.push_str("\nNext steps:\n");
    for step in steps {
        output.push_str(&format!(
            "  {}\n    {}\n",
            step.get("command")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            step.get("description")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        ));
    }
}

fn money(value: Option<&Value>) -> Option<String> {
    money::format_value(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_view_shows_product_essentials_and_checkout_action() {
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
        let output = render_human(&human_response(&response, &next_actions(&response, "test")));

        assert!(output.contains("Product (ID: product-1)"));
        assert!(output.contains("USD 71.88"));
        assert!(output.contains("checkout create"));
        assert!(!output.contains("do_not_render"));
    }
}
