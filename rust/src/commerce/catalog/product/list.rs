//! `gddy commerce catalog product list` — the products in a store.
//!
//! One page at a time, with the engine's cursor pagination: `--limit` sets the
//! page size and `--continue` resumes after the previous page. The API's own
//! cursor is what moves forward, so nothing before the page asked for is read.

use cli_engine::{
    Alignment, CliCoreError, CommandResult, CommandSpec, CursorConfig, CursorContinuation, Result,
    RuntimeCommandSpec, TableColumn, Tier,
};
use serde_json::{Value, to_value};

use super::output::ProductSummary;
use crate::{
    commerce::{
        catalog::{client::CatalogClient, connect},
        store::StoreIdArg,
    },
    error::GddyError,
    next_action::next_action,
    output_schema::output_schema,
    scopes::COMMERCE_PRODUCT_READ,
};

/// Products on a page when `--limit` is not given.
const DEFAULT_LIMIT: i64 = 25;

/// The most `--limit` accepts: the most the API returns in one request.
const MAX_LIMIT: i64 = 100;

/// The statuses a product can have, in the form the API takes them.
const STATUSES: [&str; 3] = ["DRAFT", "ACTIVE", "ARCHIVED"];

output_schema!(ProductListItem {
    "id": "string";
    "name": "string";
    "label": "string";
    "status": "string";
    "type": "string";
    "description": "string", optional;
    "variantCount": "number", optional;
    "priceMin": "number", optional;
    "priceMax": "number", optional;
    "compareAtPriceMin": "number", optional;
    "compareAtPriceMax": "number", optional;
    "createdAt": "string";
    "updatedAt": "string";
});

/// The columns `list` shows people, headed in merchant terms.
fn view_columns() -> Vec<TableColumn> {
    vec![
        TableColumn::new("id", "ID"),
        TableColumn::new("label", "Name"),
        TableColumn::new("status", "Status"),
        TableColumn::new("type", "Type"),
        TableColumn::new("variantCount", "Variants").align(Alignment::Right),
        TableColumn::new("priceMin", "Price from").align(Alignment::Right),
        TableColumn::new("priceMax", "Price to").align(Alignment::Right),
        TableColumn::new("compareAtPriceMin", "Compare-at from").align(Alignment::Right),
        TableColumn::new("compareAtPriceMax", "Compare-at to").align(Alignment::Right),
        TableColumn::new("createdAt", "Created"),
        TableColumn::new("updatedAt", "Updated"),
    ]
}

#[derive(Debug, Clone, clap::Args)]
struct ListArgs {
    #[command(flatten)]
    store: StoreIdArg,

    /// Only products with this status (repeatable): draft, active or archived.
    /// Without it, products of every status are listed.
    #[arg(long, value_name = "STATUS", value_parser = parse_status)]
    status: Vec<String>,
}

/// One `--status` value as the API takes it. Case does not matter, so `active`
/// and `ACTIVE` both work.
fn parse_status(value: &str) -> std::result::Result<String, String> {
    let upper = value.trim().to_ascii_uppercase();
    if STATUSES.contains(&upper.as_str()) {
        Ok(upper)
    } else {
        Err(format!(
            "{value:?} is not a product status; use draft, active or archived"
        ))
    }
}

/// The `--status` values as one comparable string: sorted, without repeats, so
/// `--status draft --status active` and `--status active --status draft` are the
/// same filter. Empty for no filter.
fn statuses_key(statuses: &[String]) -> String {
    let mut sorted: Vec<&str> = statuses.iter().map(String::as_str).collect();
    sorted.sort_unstable();
    sorted.dedup();
    sorted.join(",")
}

/// The `--continue` token for the page after `cursor`: the API's cursor, with the
/// filter it was issued for in front of it. A cursor means a position in one
/// particular list, so it must not be reused with another filter, and putting the
/// filter in the token is how a changed `--status` is noticed.
fn issue_token(statuses: &[String], cursor: &str) -> String {
    format!("{}|{cursor}", statuses_key(statuses))
}

/// The error for a `--continue` token that is not one this command issued.
fn bad_token() -> CliCoreError {
    GddyError::validation("the --continue token is not one `product list` printed")
        .with_fix(
            "Copy the token exactly as the previous page printed it, or leave \
            --continue off to start from the first page.",
        )
        .with_system("commerce")
        .into_cli_error()
}

/// The API cursor to resume from for `--continue token`, or `None` to start from
/// the first page. A blank token means the start; anything else has to be one this
/// command issued, for the same `--status` filter, or it is an error and not a
/// quiet return to page one.
fn resume_cursor(token: Option<&str>, statuses: &[String]) -> Result<Option<String>> {
    let Some(token) = token.map(str::trim).filter(|token| !token.is_empty()) else {
        return Ok(None);
    };
    let (issued_for, cursor) = token.split_once('|').ok_or_else(bad_token)?;
    let known = issued_for.is_empty()
        || issued_for
            .split(',')
            .all(|status| STATUSES.contains(&status));
    if !known || cursor.is_empty() {
        return Err(bad_token());
    }
    if issued_for != statuses_key(statuses) {
        let was = if issued_for.is_empty() {
            "no --status".to_owned()
        } else {
            format!(
                "--status {}",
                issued_for.to_lowercase().replace(',', " --status ")
            )
        };
        return Err(GddyError::validation(format!(
            "the --continue token was issued for a list with {was}, which is not this list"
        ))
        .with_fix(
            "Pass the same --status values as the previous page, or leave \
            --continue off to start a new list from the first page.",
        )
        .with_system("commerce")
        .into_cli_error());
    }
    Ok(Some(cursor.to_owned()))
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<ListArgs, _, _, _>(
        CommandSpec::from_args::<ListArgs>("list", "List the products in your store")
            .with_long(
                "Lists the products in your store.\n\
                 \n\
                 A page of 25 is shown at a time. --limit sets the page size (up \
                 to 100), and when there is more, the output ends with the \
                 command for the next page, which adds --continue with a token. \
                 Pass the same --status values with it.\n\
                 \n\
                 Examples:\n  \
                 gddy commerce catalog product list\n  \
                 gddy commerce catalog product list --status active --status draft --limit 100",
            )
            .with_system("commerce")
            .with_tier(Tier::Read)
            .with_default_fields("id,label,status,type,variantCount,priceMin,priceMax")
            .with_output_schema::<ProductListItem>()
            .with_view(view_columns())
            .with_scopes(&[COMMERCE_PRODUCT_READ])
            .with_cursor(CursorConfig::new(DEFAULT_LIMIT, MAX_LIMIT)),
        |ctx, args: ListArgs| async move {
            // Checked first, so a bad token fails before anything is requested.
            let after = resume_cursor(ctx.middleware.continue_token.as_deref(), &args.status)?;
            let client = connect(&ctx, &args.store, &[COMMERCE_PRODUCT_READ]).await?;
            let (products, continuation) = list_page(
                &client,
                &args.status,
                ctx.middleware.cursor_limit,
                after.as_deref(),
            )
            .await?;
            let data: Vec<Value> = products
                .into_iter()
                .map(to_value)
                .collect::<std::result::Result<_, _>>()
                .map_err(|_| {
                    GddyError::unexpected("the product list could not be shown")
                        .with_system("commerce")
                        .into_cli_error()
                })?;
            Ok(list_result(data, continuation))
        },
    )
}

/// The result of a list: the page, where it leaves off, and, only when there is a
/// product to show, a next step that uses one.
fn list_result(products: Vec<Value>, continuation: CursorContinuation) -> CommandResult {
    let offer = !products.is_empty();
    let result = CommandResult::new(Value::Array(products)).with_cursor(continuation);
    if offer {
        result.with_next_actions(vec![next_action(
            "commerce catalog product get <product-id>",
            "Show one product",
        )])
    } else {
        result
    }
}

/// One page of products, `limit` at most, after `after`, and where it ends. The
/// continuation carries the token for the next page when the API says there is
/// one, and the total number of products the API reports.
async fn list_page(
    client: &CatalogClient,
    statuses: &[String],
    limit: i64,
    after: Option<&str>,
) -> Result<(Vec<ProductSummary>, CursorContinuation)> {
    let filter = (!statuses.is_empty()).then_some(statuses);
    let Some(page) = client.list_products(limit, after, filter).await? else {
        return Ok((Vec::new(), CursorContinuation::done()));
    };
    let products: Vec<ProductSummary> = page
        .edges
        .unwrap_or_default()
        .into_iter()
        .flatten()
        .filter_map(|edge| edge.node)
        .map(ProductSummary::from)
        .collect();
    // The client has already refused a next page that has no cursor.
    let continuation = match page.page_info.end_cursor {
        Some(cursor) if page.page_info.has_next_page => {
            CursorContinuation::more(issue_token(statuses, &cursor))
        }
        _ => CursorContinuation::done(),
    };
    Ok((products, continuation.with_total(page.total_count)))
}

#[cfg(test)]
mod tests;
