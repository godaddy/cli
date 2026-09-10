use cli_engine::{CommandResult, CommandSpec, ModuleContext, Result, RuntimeCommandSpec, Tier};
use serde_json::{Value, json};

use crate::next_action::{next_action, required_value};
use crate::output_schema::output_schema;
use crate::shopping::common::{
    client_err, currency_code, make_client, merge_context_currency, read_json,
};
use crate::shopping::money;
use crate::shopping::{SHOPPING_SCOPES, command_for_env};

output_schema!(CatalogSearchOutput {
    "products": "[]object";
    "pagination": "object", optional;
    "messages": "[]object", optional;
});

#[derive(Debug, Clone, clap::Args)]
struct Args {
    /// Search request as raw JSON. Use `{}` to browse all products.
    #[arg(long, value_name = "JSON", required_unless_present = "file")]
    body: Option<String>,

    /// Path to a JSON search request. Takes precedence over --body.
    #[arg(long, value_name = "PATH")]
    file: Option<String>,

    /// Preferred ISO 4217 currency for returned catalog prices (for example, USD or GBP).
    #[arg(long, value_name = "CODE", value_parser = currency_code)]
    currency: Option<String>,

    /// Maximum number of products to return (1-100).
    #[arg(long, value_name = "N", value_parser = clap::value_parser!(u8).range(1..=100))]
    limit: Option<u8>,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<Args, _, _, _>(
        CommandSpec::from_args::<Args>("search", "Search the Shopping catalog")
            .with_long(
                "Search the Shopping catalog. Human output groups purchasable variants under each \
                 product. Use --output json to receive the unmodified Shopping API response. Supply the \
                 complete UCP search request with --body or --file; use `{}` to browse all products. \
                 Place `pagination.limit` and the response cursor in the request body to retrieve later \
                 pages with the same search criteria. Use `gddy shopping catalog get` for a selected \
                 product's complete record.",
            )
            .with_system("shopping")
            .with_tier(Tier::Read)
            .with_scopes(SHOPPING_SCOPES)
            .with_output_schema::<CatalogSearchOutput>()
            .with_view_id(HUMAN_VIEW_ID),
        |ctx, args: Args| async move {
            let mut request = read_json(args.body.as_deref(), args.file.as_deref(), "object")?;
            merge_context_currency(&mut request, args.currency.as_deref())?;
            merge_pagination(&mut request, args.limit)?;
            let client = make_client(&ctx).await?;
            let response = client.catalog_search(request.clone()).await.map_err(client_err)?;
            let next_actions = next_actions(&response, &mut request, &ctx.middleware.env)?;
            let output = if ctx.middleware.output_format == "human" {
                human_response(&response, &next_actions)
            } else {
                response
            };
            Ok(CommandResult::new(output).with_next_actions(next_actions))
        },
    )
}

const HUMAN_VIEW_ID: &str = "shopping-catalog-search";

pub(crate) fn register_human_view(ctx: &mut ModuleContext<'_>) {
    ctx.middleware_mut()
        .human_views
        .register_func(HUMAN_VIEW_ID, render_human);
}

fn human_response(response: &Value, actions: &[cli_engine::NextAction]) -> Value {
    let products = response
        .get("products")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let variant_count = products
        .iter()
        .map(|product| purchasable_variants(product).len())
        .sum::<usize>();
    let total = response
        .pointer("/pagination/total_count")
        .and_then(Value::as_u64)
        .unwrap_or(products.len() as u64);
    json!({
        "summary": format!(
            "Showing {} of {total} products · {variant_count} purchasable variants",
            products.len()
        ),
        "products": products
            .iter()
            .enumerate()
            .map(|(index, product)| json!({
                "number": index + 1,
                "id": product.get("id").and_then(Value::as_str).unwrap_or_default(),
                "title": product.get("title").and_then(Value::as_str).unwrap_or("Untitled product"),
                "variants": purchasable_variants(product)
                    .iter()
                    .map(|variant| json!({
                        "id": variant.get("id").and_then(Value::as_str).unwrap_or_default(),
                        "title": variant.get("title").and_then(Value::as_str).unwrap_or_default(),
                        "category": category(product),
                        "price": money(variant.get("price")),
                        "list_price": money(variant.get("list_price")),
                        "availability": availability(variant),
                    }))
                    .collect::<Vec<_>>(),
            }))
            .collect::<Vec<_>>(),
        "next_steps": actions
            .iter()
            .map(|action| json!({
                "command": action.command,
                "description": action.description,
            }))
            .collect::<Vec<_>>(),
    })
}

fn merge_pagination(request: &mut Value, limit: Option<u8>) -> Result<()> {
    if limit.is_none() {
        return Ok(());
    }
    let object = request
        .as_object_mut()
        .expect("read_json validates the request is an object");
    let pagination = object.entry("pagination").or_insert_with(|| json!({}));
    let pagination = pagination.as_object_mut().ok_or_else(|| {
        crate::error::GddyError::validation("pagination must be a JSON object").into_cli_error()
    })?;

    if let Some(limit) = limit {
        if let Some(existing) = pagination.get("limit")
            && existing.as_u64() != Some(u64::from(limit))
        {
            return Err(crate::error::GddyError::validation(
                "--limit conflicts with pagination.limit in the request body",
            )
            .into_cli_error());
        }
        pagination.insert("limit".to_owned(), json!(limit));
    }
    Ok(())
}

fn next_actions(
    response: &Value,
    request: &mut Value,
    env: &str,
) -> Result<Vec<cli_engine::NextAction>> {
    let mut actions = product_actions(response, request, env);
    actions.extend(next_page_action(response, request, env)?);
    Ok(actions)
}

fn product_actions(response: &Value, request: &Value, env: &str) -> Vec<cli_engine::NextAction> {
    let Some(product) = response
        .get("products")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
    else {
        return Vec::new();
    };
    let Some(product_id) = product.get("id").and_then(Value::as_str) else {
        return Vec::new();
    };
    let mut product_request = json!({"id": product_id});
    if let Some(currency) = request.pointer("/context/currency").and_then(Value::as_str) {
        product_request["context"] = json!({"currency": currency});
    }
    let product_body = product_request.to_string();
    let mut actions = vec![next_action(
        command_for_env(env, format!("catalog get --body '{product_body}'")),
        "View the selected product's complete record",
    )];
    if let Some(variant_id) = product
        .get("variants")
        .and_then(Value::as_array)
        .and_then(|variants| variants.iter().find(|variant| is_available(variant)))
        .and_then(|variant| variant.get("id"))
        .and_then(Value::as_str)
    {
        let currency = product
            .get("variants")
            .and_then(Value::as_array)
            .and_then(|variants| {
                variants
                    .iter()
                    .find(|variant| variant.get("id").and_then(Value::as_str) == Some(variant_id))
            })
            .and_then(|variant| variant.pointer("/price/currency"))
            .and_then(Value::as_str)
            .unwrap_or("USD");
        actions.push(
            next_action(
                command_for_env(
                    env,
                    format!("checkout create --item '{variant_id}' --currency {currency}"),
                ),
                "Create a checkout with the first available variant",
            )
            .with_param("variant_id", required_value(variant_id)),
        );
    }
    actions
}

fn next_page_action(
    response: &Value,
    request: &mut Value,
    env: &str,
) -> Result<Vec<cli_engine::NextAction>> {
    let Some(pagination) = response.get("pagination") else {
        return Ok(Vec::new());
    };
    if !pagination
        .get("has_next_page")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Ok(Vec::new());
    }
    let cursor = pagination
        .get("cursor")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            crate::error::GddyError::unexpected(
                "catalog search indicated another page but did not return a cursor",
            )
            .into_cli_error()
        })?;
    let request = request
        .as_object_mut()
        .expect("read_json validates the request is an object");
    let pagination = request.entry("pagination").or_insert_with(|| json!({}));
    let pagination = pagination.as_object_mut().ok_or_else(|| {
        crate::error::GddyError::validation("pagination must be a JSON object").into_cli_error()
    })?;
    pagination.insert("cursor".to_owned(), json!(cursor));
    let encoded_request = serde_json::to_string(request).map_err(|error| {
        crate::error::GddyError::unexpected(format!("failed to encode next-page request: {error}"))
            .into_cli_error()
    })?;
    Ok(vec![next_action(
        command_for_env(env, format!("catalog search --body '{encoded_request}'")),
        "Fetch the next catalog page",
    )])
}

fn render_human(response: &Value) -> String {
    let mut output = response
        .get("summary")
        .and_then(Value::as_str)
        .map_or_else(String::new, |summary| format!("{summary}\n"));
    for product in response
        .get("products")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        output.push('\n');
        output.push_str(&format!(
            "{}. {} (ID: {})\n{}\n",
            product
                .get("number")
                .and_then(Value::as_u64)
                .unwrap_or_default(),
            product
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or("Untitled product"),
            product
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            "─".repeat(72)
        ));
        output.push_str(&render_variants_table(product));
    }
    let next_steps = response
        .get("next_steps")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    if !next_steps.is_empty() {
        output.push_str("\nNext steps:\n");
        for step in next_steps {
            let command = step
                .get("command")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let description = step
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or_default();
            output.push_str(&format!("  {command}\n    {description}\n"));
        }
    }
    output
}

fn render_variants_table(product: &Value) -> String {
    let rows = product
        .get("variants")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|variant| {
            vec![
                variant
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                variant
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                variant
                    .get("category")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                variant
                    .get("price")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                variant
                    .get("list_price")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                variant
                    .get("availability")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
            ]
        })
        .collect::<Vec<_>>();
    render_table(
        &[
            "ID",
            "Description",
            "Category",
            "Your Price",
            "List Price",
            "Availability",
        ],
        &rows,
        &[false, false, false, true, true, false],
    )
}

fn render_table(headers: &[&str], rows: &[Vec<String>], right_aligned: &[bool]) -> String {
    if rows.is_empty() {
        return "No purchasable variants returned.\n".to_owned();
    }
    let widths = headers
        .iter()
        .enumerate()
        .map(|(index, header)| {
            rows.iter()
                .filter_map(|row| row.get(index))
                .map(String::len)
                .max()
                .unwrap_or_default()
                .max(header.len())
        })
        .collect::<Vec<_>>();
    let mut output = format_row(
        headers.iter().map(|header| (*header).to_owned()).collect(),
        &widths,
        right_aligned,
    );
    output.push_str(&format_row(
        widths.iter().map(|width| "-".repeat(*width)).collect(),
        &widths,
        right_aligned,
    ));
    for row in rows {
        output.push_str(&format_row(row.clone(), &widths, right_aligned));
    }
    output
}

fn format_row(values: Vec<String>, widths: &[usize], right_aligned: &[bool]) -> String {
    let cells = values
        .iter()
        .enumerate()
        .map(|(index, value)| {
            if right_aligned.get(index).copied().unwrap_or(false) {
                format!("{value:>width$}", width = widths[index])
            } else {
                format!("{value:<width$}", width = widths[index])
            }
        })
        .collect::<Vec<_>>();
    format!("{}\n", cells.join("  "))
}

fn purchasable_variants(product: &Value) -> Vec<&Value> {
    product
        .get("variants")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|variant| variant.get("id").and_then(Value::as_str).is_some())
        .collect()
}

fn is_available(variant: &Value) -> bool {
    variant
        .pointer("/availability/available")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn availability(variant: &Value) -> &'static str {
    if is_available(variant) {
        "Available"
    } else {
        "Unavailable"
    }
}

fn category(product: &Value) -> String {
    product
        .get("categories")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|category| category.get("value").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join(",")
}

fn money(value: Option<&Value>) -> Option<String> {
    money::format_value(value)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{human_response, merge_pagination, next_actions, render_human};
    use crate::shopping::command_for_env;
    use crate::shopping::common::{currency_code, merge_context_currency};

    fn response() -> serde_json::Value {
        json!({
            "products": [{
                "id": "product-1",
                "title": "Product",
                "categories": [{"value": "email"}],
                "variants": [{
                    "id": "product-1:1yr",
                    "title": "Product — 1 Year",
                    "availability": {"available": true},
                    "price": {"amount": 7188, "currency": "USD"},
                    "list_price": {"amount": 11988, "currency": "USD"}
                }]
            }],
            "pagination": {"cursor": "next", "has_next_page": true, "total_count": 14},
            "messages": []
        })
    }

    #[test]
    fn merges_limit_without_changing_a_body_cursor() {
        let mut request = json!({"query": "email", "pagination": {"cursor": "cursor-1"}});
        merge_pagination(&mut request, Some(25)).expect("valid pagination");
        assert_eq!(
            request,
            json!({"query": "email", "pagination": {"limit": 25, "cursor": "cursor-1"}})
        );
    }

    #[test]
    fn next_actions_keep_the_selected_environment() {
        let response = response();
        let mut request = json!({"pagination": {"limit": 3}});
        let actions = next_actions(&response, &mut request, "test").expect("actions");
        assert_eq!(actions.len(), 3);
        assert!(
            actions
                .iter()
                .all(|action| action.command.contains("gddy --env test"))
        );
        assert!(
            actions[0]
                .command
                .contains("catalog get --body '{\"id\":\"product-1\"}'")
        );
        assert!(actions[1].command.contains("checkout create"));
        assert!(actions[2].command.contains("\"cursor\":\"next\""));
    }

    #[test]
    fn human_output_groups_variants_by_product_with_summary() {
        let response = response();
        let actions = next_actions(&response, &mut json!({}), "test").expect("actions");
        let rendered = render_human(&human_response(&response, &actions));
        assert!(rendered.contains("Showing 1 of 14 products · 1 purchasable variants"));
        assert!(
            rendered.contains("1. Product (ID: product-1)"),
            "{rendered}"
        );
        assert!(!rendered.contains("PRODUCT ID"));
        assert!(
            rendered.contains("ID             Description"),
            "{rendered}"
        );
        assert!(rendered.contains("USD 71.88"));
        assert!(rendered.contains("Available"));
        assert!(rendered.contains("Next steps:"), "{rendered}");
        assert!(
            rendered.contains("gddy --env test shopping catalog get"),
            "{rendered}"
        );
    }

    #[test]
    fn product_commands_preserve_non_production_environment() {
        assert_eq!(
            command_for_env("prod", "catalog search"),
            "shopping catalog search"
        );
        assert_eq!(
            command_for_env("test", "catalog search"),
            "--env test shopping catalog search"
        );
    }

    #[test]
    fn merges_currency_and_preserves_it_for_follow_up_actions() {
        let response = response();
        let mut request = json!({"pagination": {"limit": 3}});
        merge_context_currency(&mut request, Some("jpy")).expect("valid currency");
        let actions = next_actions(&response, &mut request, "test").expect("actions");

        assert_eq!(request.pointer("/context/currency"), Some(&json!("jpy")));
        assert!(actions[1].command.contains("--currency USD"));
        assert!(actions[2].command.contains("\"currency\":\"jpy\""));
    }

    #[test]
    fn rejects_conflicting_currency_in_request_body() {
        let mut request = json!({"context": {"currency": "USD"}});
        assert!(merge_context_currency(&mut request, Some("JPY")).is_err());
    }

    #[test]
    fn validates_and_normalizes_currency_codes() {
        assert_eq!(currency_code(" jpy ").expect("valid currency"), "JPY");
        assert!(currency_code("JP").is_err());
        assert!(currency_code("123").is_err());
    }
}
