use cli_engine::{CommandResult, CommandSpec, ModuleContext, RuntimeCommandSpec, Tier};
use serde_json::{Value, json};

use crate::output_schema::output_schema;
use crate::shopping::SHOPPING_SCOPES;
use crate::shopping::common::{
    client_err, currency_code, make_client, merge_context_currency, read_json,
};

output_schema!(CatalogLookupOutput {
    "ucp": "object";
    "products": "[]object";
    "messages": "[]object";
});

#[derive(Debug, Clone, clap::Args)]
struct Args {
    /// Product or variant ID to resolve. Repeat to resolve multiple IDs.
    #[arg(long, value_name = "ID", required_unless_present_any = ["body", "file"])]
    id: Vec<String>,

    /// Preferred ISO 4217 currency for returned catalog prices (for example, USD or GBP).
    #[arg(long, value_name = "CODE", value_parser = currency_code)]
    currency: Option<String>,

    /// Lookup request as raw JSON for advanced Shopping API filters and extensions.
    #[arg(long, value_name = "JSON")]
    body: Option<String>,

    /// Path to a JSON lookup request. Takes precedence over --body.
    #[arg(long, value_name = "PATH")]
    file: Option<String>,
}

const HUMAN_VIEW_ID: &str = "shopping-catalog-lookup";

pub(crate) fn register_human_view(ctx: &mut ModuleContext<'_>) {
    ctx.middleware_mut()
        .human_views
        .register_func(HUMAN_VIEW_ID, render_human);
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<Args, _, _, _>(
        CommandSpec::from_args::<Args>("lookup", "Resolve known Shopping catalog IDs")
            .with_long(
                "Resolve one or more known product or variant IDs with repeatable --id. Unknown IDs \
                 are reported in the response messages rather than failing the whole request. Use \
                 --body or --file only for advanced Shopping API fields.",
            )
            .with_system("shopping")
            .with_tier(Tier::Read)
            .with_scopes(SHOPPING_SCOPES)
            .with_output_schema::<CatalogLookupOutput>()
            .with_view_id(HUMAN_VIEW_ID),
        |ctx, args: Args| async move {
            let mut body = if args.body.is_some() || args.file.is_some() {
                read_json(args.body.as_deref(), args.file.as_deref(), "object")?
            } else {
                json!({})
            };
            if !args.id.is_empty() {
                let object = body
                    .as_object_mut()
                    .expect("catalog lookup request is an object");
                if object.contains_key("ids") {
                    return Err(crate::error::GddyError::validation(
                        "--id conflicts with ids in the request body",
                    )
                    .into_cli_error());
                }
                object.insert("ids".to_owned(), json!(args.id));
            }
            merge_context_currency(&mut body, args.currency.as_deref())?;
            let client = make_client(&ctx).await?;
            let response = client.catalog_lookup(body).await.map_err(client_err)?;
            let output = if ctx.middleware.output_format == "human" {
                human_response(&response)
            } else {
                response
            };
            Ok(CommandResult::new(output))
        },
    )
}

fn human_response(response: &Value) -> Value {
    let products = response
        .get("products")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    json!({
        "products": products
            .iter()
            .map(|product| json!({
                "id": product.get("id").and_then(Value::as_str).unwrap_or_default(),
                "title": product.get("title").and_then(Value::as_str).unwrap_or("Untitled product"),
                "category": categories(product),
                "price_range": price_range(product),
                "variants": product
                    .get("variants")
                    .and_then(Value::as_array)
                    .map(Vec::as_slice)
                    .unwrap_or_default()
                    .iter()
                    .filter_map(|variant| {
                        Some(json!({
                            "id": variant.get("id").and_then(Value::as_str)?,
                            "title": variant.get("title").and_then(Value::as_str).unwrap_or("Untitled variant"),
                            "price": crate::shopping::money::format_value(variant.get("price")),
                            "list_price": crate::shopping::money::format_value(variant.get("list_price")),
                            "available": variant.pointer("/availability/available").and_then(Value::as_bool).unwrap_or(false),
                        }))
                    })
                    .collect::<Vec<_>>(),
            }))
            .collect::<Vec<_>>(),
        "messages": response.get("messages").cloned().unwrap_or_else(|| json!([])),
    })
}

fn categories(product: &Value) -> String {
    product
        .get("categories")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|category| category.get("value").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join(", ")
}

fn price_range(product: &Value) -> Option<String> {
    let range = product.get("price_range")?;
    let min = crate::shopping::money::format_value(range.get("min"))?;
    let max = crate::shopping::money::format_value(range.get("max"))?;
    Some(if min == max {
        min
    } else {
        format!("{min}–{max}")
    })
}

fn render_human(response: &Value) -> String {
    let products = response
        .get("products")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let mut output = if products.is_empty() {
        "No products found.\n".to_owned()
    } else {
        String::new()
    };
    for product in products {
        output.push_str(&format!(
            "{} (ID: {})\n",
            product
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or("Untitled product"),
            product
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        ));
        if let Some(category) = product.get("category").and_then(Value::as_str)
            && !category.is_empty()
        {
            output.push_str(&format!("Category: {category}\n"));
        }
        if let Some(price_range) = product.get("price_range").and_then(Value::as_str) {
            output.push_str(&format!("Price range: {price_range}\n"));
        }
        output.push_str("Variants:\n");
        for variant in product
            .get("variants")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let availability = if variant
                .get("available")
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
                variant
                    .get("price")
                    .and_then(Value::as_str)
                    .unwrap_or("Unavailable"),
                variant
                    .get("list_price")
                    .and_then(Value::as_str)
                    .unwrap_or("Unavailable"),
            ));
        }
        output.push('\n');
    }
    let messages = response
        .get("messages")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    for message in messages {
        if let Some(content) = message.get("content").and_then(Value::as_str) {
            output.push_str(&format!("Message: {content}\n"));
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_view_shows_lookup_essentials_without_ucp_metadata() {
        let output = render_human(&human_response(&json!({
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
        })));

        assert!(output.contains("Product (ID: product-1)"));
        assert!(output.contains("USD 71.88"));
        assert!(!output.contains("do_not_render"));
    }
}
