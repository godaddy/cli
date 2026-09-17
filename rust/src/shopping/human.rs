use cli_engine::ModuleContext;
use serde_json::{Value, json};

use crate::shopping::money;

pub(crate) const CATALOG_SEARCH_VIEW_ID: &str = "shopping-catalog-search";
pub(crate) const CATALOG_CATEGORIES_VIEW_ID: &str = "shopping-catalog-categories";
pub(crate) const CATALOG_GET_VIEW_ID: &str = "shopping-catalog-get";
pub(crate) const CATALOG_LOOKUP_VIEW_ID: &str = "shopping-catalog-lookup";
pub(crate) const CHECKOUT_VIEW_ID: &str = "shopping-checkout-get";
pub(crate) const CHECKOUT_COMPLETE_VIEW_ID: &str = "shopping-checkout-complete";
pub(crate) const ORDER_VIEW_ID: &str = "shopping-order-get";

pub(crate) fn register_human_views(ctx: &mut ModuleContext<'_>) {
    let views = &mut ctx.middleware_mut().human_views;
    views.register_func(CATALOG_SEARCH_VIEW_ID, render_catalog_search);
    views.register_func(CATALOG_CATEGORIES_VIEW_ID, render_catalog_categories);
    views.register_func(CATALOG_GET_VIEW_ID, render_catalog_product);
    views.register_func(CATALOG_LOOKUP_VIEW_ID, render_catalog_products);
    views.register_func(CHECKOUT_VIEW_ID, render_checkout);
    views.register_func(CHECKOUT_COMPLETE_VIEW_ID, render_checkout_completion);
    views.register_func(ORDER_VIEW_ID, render_order);
}

pub(crate) fn catalog_search_response(response: &Value) -> Value {
    project_catalog_products(response)
}

pub(crate) fn catalog_product_response(response: &Value) -> Value {
    project_catalog_product(response.get("product").unwrap_or(&Value::Null))
}

pub(crate) fn catalog_lookup_response(response: &Value) -> Value {
    project_catalog_products(response)
}

fn project_catalog_products(response: &Value) -> Value {
    let products = response
        .get("products")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .map(project_catalog_product)
        .collect::<Vec<_>>();
    json!({
        "products": products,
        "total_count": response.pointer("/pagination/total_count").and_then(Value::as_u64),
        "messages": response.get("messages").cloned().unwrap_or_else(|| json!([])),
    })
}

fn project_catalog_product(product: &Value) -> Value {
    json!({
        "id": product.get("id").and_then(Value::as_str).unwrap_or_default(),
        "title": product.get("title").and_then(Value::as_str).unwrap_or("Untitled product"),
        "description": product.pointer("/description/plain").and_then(Value::as_str),
        "categories": catalog_categories(product),
        "highlights": catalog_highlights(product),
        "price_range": catalog_price_range(product),
        "variants": product.get("variants").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default().iter().filter_map(project_catalog_variant).collect::<Vec<_>>(),
    })
}

fn project_catalog_variant(variant: &Value) -> Option<Value> {
    let id = variant.get("id").and_then(Value::as_str)?;
    Some(json!({
        "id": id,
        "title": variant.get("title").and_then(Value::as_str).unwrap_or("Untitled purchase option"),
        "description": variant.pointer("/description/plain").and_then(Value::as_str),
        "options": catalog_options(variant),
        "highlights": catalog_highlights(variant),
        "price": money::format_value(variant.get("price")),
        "renewal_price": money::format_value(variant.get("renewal_price")),
        "list_price": money::format_value(variant.get("list_price")),
        "available": variant.pointer("/availability/available").and_then(Value::as_bool).unwrap_or(false),
    }))
}

fn catalog_categories(product: &Value) -> Vec<&str> {
    product
        .get("categories")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|category| category.get("value").and_then(Value::as_str))
        .collect()
}

fn catalog_price_range(product: &Value) -> Option<String> {
    let range = product.get("price_range")?;
    let min = money::format_value(range.get("min"))?;
    let max = money::format_value(range.get("max"))?;
    Some(if min == max {
        min
    } else {
        format!("{min}–{max}")
    })
}

const HIGHLIGHT_LIMIT: usize = 4;

fn catalog_highlights(value: &Value) -> Vec<String> {
    value
        .get("tags")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

fn catalog_options(variant: &Value) -> Vec<String> {
    variant
        .get("options")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|option| {
            let name = option.get("name").and_then(Value::as_str)?;
            let label = option.get("label").and_then(Value::as_str)?;
            Some(format!("{name}: {label}"))
        })
        .collect()
}

fn render_catalog_categories(response: &Value) -> String {
    let categories = response
        .get("categories")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    if categories.is_empty() {
        return "No supported product categories found.\n".to_owned();
    }
    format!(
        "Supported product categories:\n{}\n",
        categories
            .iter()
            .filter_map(Value::as_str)
            .map(|category| format!("- {category}"))
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

fn render_catalog_search(response: &Value) -> String {
    let products = response
        .get("products")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    if products.is_empty() {
        return "No products found.\n".to_owned();
    }

    let total_count = response
        .get("total_count")
        .and_then(Value::as_u64)
        .unwrap_or(products.len() as u64);
    let mut output = format!("Showing {} of {total_count} products\n\n", products.len());
    for (index, product) in products.iter().enumerate() {
        if index > 0 {
            output.push_str("\n--------------\n\n");
        }
        output.push_str(&format!(
            "{}. {}",
            index + 1,
            render_catalog_search_product(product)
        ));
    }
    render_messages(&mut output, response);
    output
}

fn render_catalog_search_product(product: &Value) -> String {
    let mut output = String::new();
    render_catalog_product_summary(&mut output, product);
    output.push_str("\nPurchase Options:\n");
    let variants = product
        .get("variants")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    if variants.is_empty() {
        output.push_str("- None\n");
    }
    for variant in variants {
        output.push_str(&format!(
            "- {}\n  ID: {}\n  Your price: {}\n  List price: {}\n",
            text(variant, "title", "Untitled purchase option"),
            text(variant, "id", ""),
            optional_text(variant, "price", "Unavailable"),
            optional_text(variant, "list_price", "Unavailable"),
        ));
    }
    output
}

fn render_catalog_product(product: &Value) -> String {
    let mut output = String::new();
    render_catalog_product_summary(&mut output, product);
    render_highlights(&mut output, product);
    output.push_str("Purchase Options:\n");
    let variants = product
        .get("variants")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    if variants.is_empty() {
        output.push_str("- None\n");
    }
    for variant in variants {
        render_catalog_variant_detail(&mut output, variant);
    }
    output
}

fn render_catalog_variant_detail(output: &mut String, variant: &Value) {
    output.push_str(&format!(
        "- {} (ID: {})\n  {}\n",
        text(variant, "title", "Untitled purchase option"),
        text(variant, "id", ""),
        catalog_pricing(variant),
    ));
    if let Some(description) = variant.get("description").and_then(Value::as_str) {
        output.push_str(&format!("  {description}\n"));
    }
    render_options(output, variant, "  ");
    render_highlights_with_indent(output, variant, "  ");
}

fn render_catalog_products(response: &Value) -> String {
    let products = response
        .get("products")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    if products.is_empty() {
        return "No products found.\n".to_owned();
    }
    let mut output = products
        .iter()
        .map(render_catalog_product)
        .collect::<Vec<_>>()
        .join("\n");
    render_messages(&mut output, response);
    output
}

fn render_catalog_product_summary(output: &mut String, product: &Value) {
    output.push_str(&format!(
        "{} (ID: {})\n",
        text(product, "title", "Untitled product"),
        text(product, "id", ""),
    ));
    if let Some(description) = product.get("description").and_then(Value::as_str) {
        output.push_str(&format!("{description}\n"));
    }
    let categories = product
        .get("categories")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    if !categories.is_empty() {
        output.push_str(&format!("Category: {}\n", categories.join(", ")));
    }
    if let Some(price_range) = product.get("price_range").and_then(Value::as_str) {
        output.push_str(&format!("Price range: {price_range}\n"));
    }
}

fn catalog_pricing(variant: &Value) -> String {
    let price = optional_text(variant, "price", "Unavailable");
    let renewal_price = variant.get("renewal_price").and_then(Value::as_str);
    let list_price = variant.get("list_price").and_then(Value::as_str);
    let mut values = vec![format!("Your price: {price}")];
    if renewal_price.is_some_and(|renewal| renewal != price) {
        values.push(format!("Renews: {}", renewal_price.unwrap_or_default()));
    }
    if list_price.is_some_and(|list| list != price && Some(list) != renewal_price) {
        values.push(format!("List price: {}", list_price.unwrap_or_default()));
    }
    if variant
        .get("available")
        .and_then(Value::as_bool)
        .is_some_and(|available| !available)
    {
        values.push("Unavailable".to_owned());
    }
    values.join(" · ")
}

fn render_options(output: &mut String, value: &Value, indent: &str) {
    let options = value
        .get("options")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    if !options.is_empty() {
        output.push_str(&format!(
            "{indent}{}\n",
            options
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(" · ")
        ));
    }
}

fn render_highlights(output: &mut String, value: &Value) {
    render_highlights_with_indent(output, value, "");
}

fn render_highlights_with_indent(output: &mut String, value: &Value, indent: &str) {
    let highlights = value
        .get("highlights")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    if highlights.is_empty() {
        return;
    }
    let shown = highlights
        .iter()
        .take(HIGHLIGHT_LIMIT)
        .copied()
        .collect::<Vec<_>>();
    output.push_str(&format!("{indent}Highlights: {}", shown.join(" · ")));
    if highlights.len() > HIGHLIGHT_LIMIT {
        output.push_str(&format!(" · +{} more", highlights.len() - HIGHLIGHT_LIMIT));
    }
    output.push('\n');
}

fn render_messages(output: &mut String, response: &Value) {
    for message in response
        .get("messages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(content) = message.get("content").and_then(Value::as_str) {
            output.push_str(&format!("Message: {content}\n"));
        }
    }
}

pub(crate) fn checkout_response(checkout: &Value, show_all_payment_instruments: bool) -> Value {
    let line_items = checkout
        .get("line_items")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|item| {
                    json!({
                        "quantity": item.get("quantity").and_then(Value::as_u64).unwrap_or(1),
                        "title": item.pointer("/item/title").and_then(Value::as_str).unwrap_or("Unknown item"),
                        "included": item.get("included_products").and_then(Value::as_array).map(|products| products.iter().filter_map(|product| product.get("title").and_then(Value::as_str)).collect::<Vec<_>>()).unwrap_or_default(),
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    json!({
        "id": checkout.get("id").and_then(Value::as_str).unwrap_or_default(),
        "status": checkout.get("status").and_then(Value::as_str).unwrap_or_default(),
        "items": line_items,
        "total": money::format_total(checkout.get("totals"), checkout.get("currency").and_then(Value::as_str)),
        "buyer": checkout_buyer(checkout),
        "selected_payment": selected_payment(checkout),
        "available_payment_instruments": available_payment_instruments(checkout, show_all_payment_instruments),
        "has_more_payment_instruments": !show_all_payment_instruments && available_payment_instrument_count(checkout) > PAYMENT_INSTRUMENT_LIMIT,
        "required_agreements": required_agreements(checkout),
        "links": checkout_links(checkout),
    })
}

const PAYMENT_INSTRUMENT_LIMIT: usize = 5;

fn checkout_buyer(checkout: &Value) -> Value {
    let buyer = checkout.get("buyer").and_then(Value::as_object);
    json!({
        "first_name": buyer.and_then(|buyer| buyer.get("first_name")).and_then(Value::as_str),
        "last_name": buyer.and_then(|buyer| buyer.get("last_name")).and_then(Value::as_str),
        "email": buyer.and_then(|buyer| buyer.get("email")).and_then(Value::as_str),
        "phone_number": buyer.and_then(|buyer| buyer.get("phone_number")).and_then(Value::as_str),
    })
}

fn available_payment_instruments(checkout: &Value, show_all: bool) -> Vec<Value> {
    checkout
        .pointer("/payment/instruments")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|instrument| {
            let id = instrument.get("id").and_then(Value::as_str)?;
            Some(json!({
                "id": id,
                "description": payment_instrument_description(instrument),
                "selected": instrument.get("selected").and_then(Value::as_bool).unwrap_or(false),
            }))
        })
        .take(if show_all {
            usize::MAX
        } else {
            PAYMENT_INSTRUMENT_LIMIT
        })
        .collect()
}

fn available_payment_instrument_count(checkout: &Value) -> usize {
    checkout
        .pointer("/payment/instruments")
        .and_then(Value::as_array)
        .map_or(0, Vec::len)
}

fn payment_instrument_description(instrument: &Value) -> &str {
    instrument
        .get("rich_text_description")
        .or_else(|| instrument.get("description"))
        .and_then(Value::as_str)
        .unwrap_or("Saved payment method")
}

fn selected_payment(checkout: &Value) -> String {
    let Some(instrument) = selected_payment_instrument(checkout) else {
        return "No payment method selected".to_owned();
    };
    let description = payment_instrument_description(instrument);
    match instrument.get("id").and_then(Value::as_str) {
        Some(id) => format!("{description} (ID: {id})"),
        None => description.to_owned(),
    }
}

fn selected_payment_instrument(checkout: &Value) -> Option<&Value> {
    checkout
        .pointer("/payment/instruments")
        .and_then(Value::as_array)
        .and_then(|instruments| {
            instruments.iter().find(|instrument| {
                instrument.get("selected").and_then(Value::as_bool) == Some(true)
            })
        })
}

fn required_agreements(checkout: &Value) -> Vec<Value> {
    checkout
        .get("required_agreements")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|agreement| {
            let key = agreement.get("key").and_then(Value::as_str)?;
            Some(json!({
                "key": key,
                "title": agreement.get("title").and_then(Value::as_str).unwrap_or(key),
                "url": agreement.get("url").and_then(Value::as_str),
                "content": agreement.get("content").and_then(Value::as_str),
                "required": agreement.get("required").and_then(Value::as_bool).unwrap_or(false),
            }))
        })
        .collect()
}

fn checkout_links(checkout: &Value) -> Vec<Value> {
    checkout
        .get("links")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|link| {
            let url = link.get("url").and_then(Value::as_str)?;
            let link_type = link.get("type").and_then(Value::as_str).unwrap_or("link");
            let title = link
                .get("title")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or_else(|| link_type.replace('_', " "));
            Some(json!({"title": title, "url": url, "type": link_type}))
        })
        .collect()
}

fn render_checkout(cart: &Value) -> String {
    if let Some(action) = cart.get("action").and_then(Value::as_str) {
        let body = cart.get("body").cloned().unwrap_or(Value::Null);
        return format!(
            "{action}\nRequest:\n{}\n",
            serde_json::to_string_pretty(&body).unwrap_or_else(|_| body.to_string()),
        );
    }
    let mut output = format!(
        "Checkout session: {}\nStatus: {}\n",
        text(cart, "id", ""),
        text(cart, "status", ""),
    );
    render_buyer(&mut output, cart);
    output.push_str("\nItems:\n");
    let items = cart
        .get("items")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    if items.is_empty() {
        output.push_str("- None\n");
    }
    for item in items {
        output.push_str(&format!(
            "- {} × {}\n",
            item.get("quantity").and_then(Value::as_u64).unwrap_or(1),
            text(item, "title", "Unknown item"),
        ));
        for included in item
            .get("included")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(included) = included.as_str() {
                output.push_str(&format!("  Includes: {included}\n"));
            }
        }
    }
    output.push_str(&format!(
        "\nSelected payment: {}\n",
        text(cart, "selected_payment", "No payment method selected")
    ));
    render_available_payment_instruments(&mut output, cart);
    if let Some(total) = cart.get("total").and_then(Value::as_str) {
        output.push_str(&format!("\nTotal: {total}\n"));
    }
    render_required_agreements(&mut output, cart);
    render_links(&mut output, cart);
    output.push_str("\nReview this checkout session and its links before placing an order.\n");
    output
}

fn render_buyer(output: &mut String, checkout: &Value) {
    let Some(buyer) = checkout.get("buyer").and_then(Value::as_object) else {
        return;
    };
    let fields = [
        ("First name", "first_name"),
        ("Last name", "last_name"),
        ("Email", "email"),
        ("Phone", "phone_number"),
    ];
    let details = fields
        .iter()
        .filter_map(|(label, key)| {
            buyer
                .get(*key)
                .and_then(Value::as_str)
                .map(|value| format!("{label}: {value}"))
        })
        .collect::<Vec<_>>();
    if !details.is_empty() {
        output.push_str(&format!("Buyer: {}\n", details.join(" · ")));
    }
}

fn render_available_payment_instruments(output: &mut String, cart: &Value) {
    let instruments = cart
        .get("available_payment_instruments")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    if instruments.is_empty() {
        return;
    }
    output.push_str("\nAvailable payment methods:\n");
    for instrument in instruments {
        let selected = if instrument
            .get("selected")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            " (Currently selected)"
        } else {
            ""
        };
        output.push_str(&format!(
            "- {} (ID: {}){selected}\n",
            text(instrument, "description", "Saved payment method"),
            text(instrument, "id", ""),
        ));
    }
    if cart
        .get("has_more_payment_instruments")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        output.push_str("Use --show-all-payment-instruments to show every saved payment method.\n");
    }
}

fn render_required_agreements(output: &mut String, cart: &Value) {
    let agreements = cart
        .get("required_agreements")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    if agreements.is_empty() {
        return;
    }
    output.push_str("\nRequired agreements:\n");
    for agreement in agreements {
        let required = if agreement
            .get("required")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            " (Required)"
        } else {
            ""
        };
        output.push_str(&format!(
            "- {} ({}): {}{required}\n",
            text(agreement, "title", "Agreement"),
            text(agreement, "key", ""),
            agreement
                .get("url")
                .and_then(Value::as_str)
                .or_else(|| agreement.get("content").and_then(Value::as_str))
                .unwrap_or("No link provided"),
        ));
    }
}

fn render_links(output: &mut String, cart: &Value) {
    let links = cart
        .get("links")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    if links.is_empty() {
        return;
    }
    output.push_str("\nImportant links:\n");
    for link in links {
        output.push_str(&format!(
            "- {}: {}\n",
            text(link, "title", "Link"),
            text(link, "url", "")
        ));
    }
}

pub(crate) fn checkout_completion_response(completion: &Value) -> Value {
    json!({
        "cart_id": completion.get("id").and_then(Value::as_str).unwrap_or_default(),
        "status": completion.get("status").and_then(Value::as_str).unwrap_or_default(),
        "order_id": completion.pointer("/order/id").and_then(Value::as_str),
        "order_permalink": completion.pointer("/order/permalink_url").and_then(Value::as_str),
        "total": money::format_total(completion.get("totals"), completion.get("currency").and_then(Value::as_str)),
    })
}

fn render_checkout_completion(completion: &Value) -> String {
    if let Some(action) = completion.get("action").and_then(Value::as_str) {
        return format!("{action}\nCart: {}\n", text(completion, "id", ""));
    }
    let mut output = format!(
        "Checkout session: {}\nStatus: {}\n",
        text(completion, "cart_id", ""),
        text(completion, "status", "")
    );
    if let Some(order_id) = completion.get("order_id").and_then(Value::as_str) {
        output.push_str(&format!("Order: {order_id}\n"));
    }
    if let Some(permalink) = completion.get("order_permalink").and_then(Value::as_str) {
        output.push_str(&format!("View order: {permalink}\n"));
    }
    if let Some(total) = completion.get("total").and_then(Value::as_str) {
        output.push_str(&format!("Total: {total}\n"));
    }
    output
}

pub(crate) fn order_response(order: &Value) -> Value {
    json!({
        "id": order.get("id").and_then(Value::as_str).unwrap_or_default(),
        "permalink_url": order.get("permalink_url").and_then(Value::as_str),
        "line_items": order.get("line_items").cloned().unwrap_or_else(|| json!([])),
        "total": money::format_total(order.get("totals"), order.get("currency").and_then(Value::as_str)),
        "currency": order.get("currency").and_then(Value::as_str),
        "fulfillment": order.get("fulfillment").cloned().unwrap_or_else(|| json!({})),
    })
}

fn render_order(order: &Value) -> String {
    let mut output = format!("Order: {}\n", text(order, "id", ""));
    if let Some(permalink) = order.get("permalink_url").and_then(Value::as_str) {
        output.push_str(&format!("View order: {permalink}\n"));
    }
    output.push_str("\nItems:\n");
    let items = order
        .get("line_items")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    if items.is_empty() {
        output.push_str("- None\n");
    }
    for item in items {
        let quantity = item
            .pointer("/quantity/total")
            .or_else(|| item.get("quantity"))
            .and_then(Value::as_u64)
            .unwrap_or(1);
        let title = item
            .pointer("/item/title")
            .and_then(Value::as_str)
            .unwrap_or("Unknown item");
        let status = item
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        output.push_str(&format!("- {quantity} × {title} · {status}\n"));
    }
    if let Some(total) = order.get("total").and_then(Value::as_str) {
        output.push_str(&format!("\nTotal: {total}\n"));
    }
    render_fulfillment(&mut output, order.get("fulfillment"));
    output
}

fn render_fulfillment(output: &mut String, fulfillment: Option<&Value>) {
    let Some(fulfillment) = fulfillment.and_then(Value::as_object) else {
        return;
    };
    let expectations = fulfillment
        .get("expectations")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    if expectations.is_empty() {
        return;
    }
    output.push_str("\nFulfillment:\n");
    for expectation in expectations {
        if let Some(status) = expectation.get("status").and_then(Value::as_str) {
            output.push_str(&format!("- {status}\n"));
        }
    }
}

fn text<'a>(value: &'a Value, key: &str, default: &'a str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or(default)
}

fn optional_text(value: &Value, key: &str, default: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or(default)
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog_response() -> Value {
        json!({
            "pagination": {"total_count": 1},
            "products": [{
                "id": "product-1",
                "title": "Product",
                "categories": [{"value": "email"}],
                "tags": ["Mailbox: 50 GB", "Apps: Office web apps"],
                "variants": [{
                    "id": "product-1:1yr",
                    "title": "One year",
                    "description": {"plain": "Annual email plan."},
                    "options": [{"name": "Term", "label": "1 year"}, {"name": "Mailbox", "label": "50 GB"}],
                    "tags": ["Includes Office web apps"],
                    "availability": {"available": true},
                    "price": {"amount": 7188, "currency": "USD"},
                    "renewal_price": {"amount": 11988, "currency": "USD"},
                    "list_price": {"amount": 11988, "currency": "USD"}
                }]
            }]
        })
    }

    #[test]
    fn catalog_search_groups_variants_without_optional_details() {
        let response = catalog_search_response(&catalog_response());
        let output = render_catalog_search(&response);

        assert_eq!(response["products"].as_array().map(Vec::len), Some(1));
        assert!(output.contains("Showing 1 of 1 products"));
        assert!(output.contains("1. Product (ID: product-1)"));
        assert!(output.contains("- One year"));
        assert!(output.contains("ID: product-1:1yr"));
        assert!(output.contains("Your price: USD 71.88"));
        assert!(output.contains("List price: USD 119.88"));
        assert!(!output.contains("Term:"));
        assert!(!output.contains("Availability:"));
    }

    #[test]
    fn catalog_detail_reuses_projection_without_ucp_metadata() {
        let response = json!({"product": catalog_response()["products"][0].clone(), "ucp": {"do_not_render": true}});
        let output = render_catalog_product(&catalog_product_response(&response));

        assert!(output.contains("Your price: USD 71.88"));
        assert!(output.contains("Renews: USD 119.88"));
        assert!(output.contains("Term: 1 year · Mailbox: 50 GB"));
        assert!(output.contains("Annual email plan."));
        assert!(output.contains("Highlights: Includes Office web apps"));
        assert!(!output.contains("do_not_render"));
    }

    #[test]
    fn catalog_lookup_uses_the_detailed_variant_renderer() {
        let output = render_catalog_products(&catalog_lookup_response(&catalog_response()));

        assert!(output.contains("Annual email plan."));
        assert!(output.contains("Term: 1 year · Mailbox: 50 GB"));
        assert!(output.contains("Highlights: Includes Office web apps"));
    }

    #[test]
    fn checkout_response_and_human_view_include_required_agreements() {
        let response = checkout_response(
            &json!({
                "id": "checkout-1",
                "status": "ready_for_complete",
                "required_agreements": [{
                    "key": "terms",
                    "title": "Terms of Service",
                    "url": "https://example.test/terms",
                    "required": true
                }]
            }),
            false,
        );
        let output = render_checkout(&response);

        assert_eq!(response["required_agreements"][0]["key"], "terms");
        assert!(output.contains("Required agreements:"));
        assert!(output.contains("Terms of Service (terms): https://example.test/terms (Required)"));
    }
}
