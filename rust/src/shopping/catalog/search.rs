use cli_engine::{
    Alignment, CommandResult, CommandSpec, HumanViewDef, ModuleContext, NextAction,
    NextActionParam, Result, RuntimeCommandSpec, TableColumn, Tier,
};
use serde_json::{Value, json};

use crate::next_action::next_action;
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
    /// Text to search for. Omit to browse the catalog.
    #[arg(long, value_name = "TEXT")]
    query: Option<String>,

    /// Product category to include. Supported values: email, pointOfSale, sslCertificate, webHosting, websiteBuilder. Repeat to include multiple categories.
    #[arg(long, value_name = "CATEGORY", value_parser = category_value)]
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

    /// Search request as raw JSON for advanced Shopping API filters and extensions.
    #[arg(long, value_name = "JSON")]
    body: Option<String>,

    /// Path to a JSON search request. Takes precedence over --body.
    #[arg(long, value_name = "PATH")]
    file: Option<String>,
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
            .with_view_id(HUMAN_VIEW_ID),
        |ctx, args: Args| async move {
            let mut request = if args.body.is_some() || args.file.is_some() {
                read_json(args.body.as_deref(), args.file.as_deref(), "object")?
            } else {
                json!({})
            };
            merge_search_args(
                &mut request,
                args.query.as_deref(),
                &args.category,
                args.cursor.as_deref(),
            )?;
            merge_context_currency(&mut request, args.currency.as_deref())?;
            merge_pagination(&mut request, args.limit)?;
            validate_price_filter_currency(&request)?;
            let client = make_client(&ctx).await?;
            let response = client
                .catalog_search(request.clone())
                .await
                .map_err(client_err)?;
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
const CATEGORIES: &[&str] = &[
    "email",
    "pointOfSale",
    "sslCertificate",
    "webHosting",
    "websiteBuilder",
];

fn category_value(value: &str) -> std::result::Result<String, String> {
    CATEGORIES
        .contains(&value)
        .then(|| value.to_owned())
        .ok_or_else(|| format!("category must be one of: {}", CATEGORIES.join(", ")))
}

pub(crate) fn register_human_view(ctx: &mut ModuleContext<'_>) {
    ctx.middleware_mut().human_views.register(HumanViewDef::new(
        HUMAN_VIEW_ID,
        vec![
            TableColumn::new("product", "Product"),
            TableColumn::new("variant", "Variant"),
            TableColumn::new("variant_id", "Variant ID").no_truncate(true),
            TableColumn::new("category", "Category"),
            TableColumn::new("price", "Your Price").align(Alignment::Right),
            TableColumn::new("list_price", "List Price").align(Alignment::Right),
            TableColumn::new("term", "Term"),
            TableColumn::new("availability", "Availability"),
        ],
    ));
}

fn human_response(response: &Value, _actions: &[NextAction]) -> Value {
    Value::Array(
        response
            .get("products")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .flat_map(|product| {
                purchasable_variants(product).into_iter().map(move |variant| {
                    json!({
                        "product": product.get("title").and_then(Value::as_str).unwrap_or("Untitled product"),
                        "variant": variant.get("title").and_then(Value::as_str).unwrap_or("Untitled variant"),
                        "variant_id": variant.get("id").and_then(Value::as_str).unwrap_or_default(),
                        "category": category(product),
                        "price": money(variant.get("price")),
                        "list_price": money(variant.get("list_price")),
                        "term": term(variant),
                        "availability": availability(variant),
                    })
                })
            })
            .collect(),
    )
}

fn merge_search_args(
    request: &mut Value,
    query: Option<&str>,
    categories: &[String],
    cursor: Option<&str>,
) -> Result<()> {
    let object = request
        .as_object_mut()
        .expect("catalog search request is an object");
    if let Some(query) = query {
        merge_string(object, "query", query, "--query")?;
    }
    if !categories.is_empty() {
        let filters = object.entry("filters").or_insert_with(|| json!({}));
        let filters = filters.as_object_mut().ok_or_else(|| {
            crate::error::GddyError::validation("filters must be a JSON object").into_cli_error()
        })?;
        if filters.contains_key("categories") {
            return Err(crate::error::GddyError::validation(
                "--category conflicts with filters.categories in the request body",
            )
            .into_cli_error());
        }
        filters.insert("categories".to_owned(), json!(categories));
    }
    if let Some(cursor) = cursor {
        let pagination = object.entry("pagination").or_insert_with(|| json!({}));
        let pagination = pagination.as_object_mut().ok_or_else(|| {
            crate::error::GddyError::validation("pagination must be a JSON object").into_cli_error()
        })?;
        merge_string(pagination, "cursor", cursor, "--cursor")?;
    }
    Ok(())
}

fn merge_string(
    object: &mut serde_json::Map<String, Value>,
    key: &str,
    value: &str,
    flag: &str,
) -> Result<()> {
    if let Some(existing) = object.get(key).and_then(Value::as_str)
        && existing != value
    {
        return Err(crate::error::GddyError::validation(format!(
            "{flag} conflicts with {key} in the request body"
        ))
        .into_cli_error());
    }
    object.insert(key.to_owned(), json!(value));
    Ok(())
}

fn validate_price_filter_currency(request: &Value) -> Result<()> {
    if request.pointer("/filters/price").is_some()
        && request
            .pointer("/context/currency")
            .and_then(Value::as_str)
            .is_none()
    {
        return Err(crate::error::GddyError::validation(
            "filters.price requires context.currency because price bounds are currency-specific minor units",
        )
        .into_cli_error());
    }
    Ok(())
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

fn next_actions(response: &Value, request: &mut Value, env: &str) -> Result<Vec<NextAction>> {
    let mut actions = product_actions(response, request, env);
    actions.extend(next_page_action(response, request, env)?);
    Ok(actions)
}

fn product_actions(response: &Value, request: &Value, env: &str) -> Vec<NextAction> {
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
    let currency = request.pointer("/context/currency").and_then(Value::as_str);
    let mut get_action = next_action(
        command_for_env(env, "catalog get --id <product-id>"),
        "View the selected product's details",
    )
    .with_param("product-id", NextActionParam::value(product_id));
    if let Some(currency) = currency {
        get_action = get_action.with_param("currency", NextActionParam::value(currency));
        get_action.command.push_str(" --currency <currency>");
    }
    let mut actions = vec![get_action];
    if let Some(variant) = product
        .get("variants")
        .and_then(Value::as_array)
        .and_then(|variants| variants.iter().find(|variant| is_available(variant)))
    {
        let Some(variant_id) = variant.get("id").and_then(Value::as_str) else {
            return actions;
        };
        let currency = variant
            .pointer("/price/currency")
            .and_then(Value::as_str)
            .unwrap_or("USD");
        actions.push(
            next_action(
                command_for_env(
                    env,
                    "checkout create --item <variant-id> --currency <currency>",
                ),
                "Add the first available variant to a cart",
            )
            .with_param("variant-id", NextActionParam::value(variant_id))
            .with_param("currency", NextActionParam::value(currency)),
        );
    }
    actions
}

fn next_page_action(response: &Value, request: &mut Value, env: &str) -> Result<Vec<NextAction>> {
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
        .expect("catalog search request is an object");
    let pagination = request.entry("pagination").or_insert_with(|| json!({}));
    let pagination = pagination.as_object_mut().ok_or_else(|| {
        crate::error::GddyError::validation("pagination must be a JSON object").into_cli_error()
    })?;
    pagination.insert("cursor".to_owned(), json!(cursor));
    Ok(vec![search_action(request, env)?])
}

fn search_action(request: &serde_json::Map<String, Value>, env: &str) -> Result<NextAction> {
    if !is_simple_search_request(request) {
        let body = serde_json::to_string(request).map_err(|error| {
            crate::error::GddyError::unexpected(format!(
                "failed to encode next-page request: {error}"
            ))
            .into_cli_error()
        })?;
        return Ok(next_action(
            command_for_env(env, "catalog search --body <body>"),
            "Fetch the next catalog page",
        )
        .with_param("body", NextActionParam::value(body)));
    }

    let mut command = "catalog search".to_owned();
    let mut params = Vec::new();
    append_search_param(&mut command, &mut params, "query", request.get("query"));
    if let Some(categories) = request
        .get("filters")
        .and_then(Value::as_object)
        .and_then(|filters| filters.get("categories"))
        .and_then(Value::as_array)
    {
        for (index, category) in categories.iter().filter_map(Value::as_str).enumerate() {
            let name = format!("category-{index}");
            command.push_str(&format!(" --category <{name}>"));
            params.push((name, category.to_owned()));
        }
    }
    let pagination = request.get("pagination").and_then(Value::as_object);
    append_search_param(
        &mut command,
        &mut params,
        "cursor",
        pagination.and_then(|pagination| pagination.get("cursor")),
    );
    if let Some(limit) = pagination
        .and_then(|pagination| pagination.get("limit"))
        .and_then(Value::as_u64)
    {
        command.push_str(&format!(" --limit {limit}"));
    }
    let context = request.get("context").and_then(Value::as_object);
    append_search_param(
        &mut command,
        &mut params,
        "currency",
        context.and_then(|context| context.get("currency")),
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
    value: Option<&Value>,
) {
    if let Some(value) = value.and_then(Value::as_str) {
        command.push_str(&format!(" --{name} <{name}>"));
        params.push((name.to_owned(), value.to_owned()));
    }
}

fn is_simple_search_request(request: &serde_json::Map<String, Value>) -> bool {
    request
        .keys()
        .all(|key| matches!(key.as_str(), "query" | "filters" | "pagination" | "context"))
        && request
            .get("filters")
            .and_then(Value::as_object)
            .is_none_or(|filters| filters.keys().all(|key| key == "categories"))
        && request
            .get("pagination")
            .and_then(Value::as_object)
            .is_none_or(|pagination| {
                pagination
                    .keys()
                    .all(|key| key == "cursor" || key == "limit")
            })
        && request
            .get("context")
            .and_then(Value::as_object)
            .is_none_or(|context| context.keys().all(|key| key == "currency"))
}

fn term(variant: &Value) -> String {
    variant
        .get("options")
        .and_then(Value::as_array)
        .and_then(|options| {
            options
                .iter()
                .find(|option| option.get("name").and_then(Value::as_str) == Some("Term"))
        })
        .and_then(|option| option.get("label"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_default()
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

    use super::{
        category_value, human_response, merge_pagination, merge_search_args, next_actions,
        search_action, validate_price_filter_currency,
    };
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
    fn builds_search_request_without_a_json_body() {
        let mut request = json!({});
        merge_search_args(
            &mut request,
            Some("email"),
            &["email".to_owned(), "hosting".to_owned()],
            Some("cursor-1"),
        )
        .expect("flags should merge");
        merge_context_currency(&mut request, Some("GBP")).expect("currency should merge");
        merge_pagination(&mut request, Some(3)).expect("limit should merge");

        assert_eq!(
            request,
            json!({
                "query": "email",
                "filters": {"categories": ["email", "hosting"]},
                "pagination": {"cursor": "cursor-1", "limit": 3},
                "context": {"currency": "GBP"}
            })
        );
    }

    #[test]
    fn requires_currency_for_raw_price_filter() {
        assert!(
            validate_price_filter_currency(&json!({"filters": {"price": {"min": 100}}})).is_err()
        );
        assert!(
            validate_price_filter_currency(&json!({
                "filters": {"price": {"min": 100}},
                "context": {"currency": "USD"}
            }))
            .is_ok()
        );
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
                .all(|action| action.command.starts_with("gddy shopping"))
        );
        assert_eq!(
            actions[0].command,
            "gddy shopping catalog get --id <product-id>"
        );
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
        let mut request = json!({
            "query": "O'Reilly",
            "filters": {"categories": ["web & email"]},
            "pagination": {"limit": 3}
        });
        let actions = next_actions(&response, &mut request, "test").expect("actions");

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
    fn advanced_next_page_keeps_json_in_a_structured_parameter() {
        let action = search_action(
            &serde_json::from_value(json!({"signals": {"value": "O'Reilly"}})).expect("object"),
            "test",
        )
        .expect("action");

        assert_eq!(action.command, "gddy shopping catalog search --body <body>");
        assert_eq!(
            action.params["body"].value.as_deref(),
            Some(r#"{"signals":{"value":"O'Reilly"}}"#)
        );
    }

    #[test]
    fn human_output_groups_variants_by_product_with_summary() {
        let response = response();
        let actions = next_actions(&response, &mut json!({}), "test").expect("actions");
        let rows = human_response(&response, &actions);
        assert_eq!(rows.as_array().map(Vec::len), Some(1));
        assert_eq!(rows[0]["product"], "Product");
        assert_eq!(rows[0]["variant_id"], "product-1:1yr");
        assert_eq!(rows[0]["price"], "USD 71.88");
        assert_eq!(rows[0]["availability"], "Available");
    }

    #[test]
    fn merges_currency_and_preserves_it_for_follow_up_actions() {
        let response = response();
        let mut request = json!({"pagination": {"limit": 3}});
        merge_context_currency(&mut request, Some("jpy")).expect("valid currency");
        let actions = next_actions(&response, &mut request, "test").expect("actions");

        assert_eq!(request.pointer("/context/currency"), Some(&json!("jpy")));
        assert_eq!(actions[1].params["currency"].value.as_deref(), Some("USD"));
        assert_eq!(actions[2].params["currency"].value.as_deref(), Some("jpy"));
    }

    #[test]
    fn rejects_conflicting_currency_in_request_body() {
        let mut request = json!({"context": {"currency": "USD"}});
        assert!(merge_context_currency(&mut request, Some("JPY")).is_err());
    }

    #[test]
    fn validates_api_derived_non_domain_categories() {
        assert_eq!(category_value("webHosting"), Ok("webHosting".to_owned()));
        assert!(category_value("domain").is_err());
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
