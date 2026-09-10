use cli_engine::{CommandResult, CommandSpec, NextActionParam, RuntimeCommandSpec, Tier};
use serde_json::{Value, json};

use crate::hosting::common::{HostingSubscriptionList, client_err, make_client, next_page_token};
use crate::next_action::next_action;
use crate::scopes::HOSTING_SUBSCRIPTION_READ as SUB_READ;

#[derive(Debug, Clone, clap::Args)]
struct SubscriptionListArgs {
    /// Filter by hosting product (WEB_HOSTING or MANAGED_WORDPRESS).
    #[arg(long, value_name = "PRODUCT")]
    hosting_product: Option<String>,

    /// Maximum number of subscriptions to return. Omit to return all.
    #[arg(long, value_name = "N", value_parser = clap::value_parser!(u32).range(1..))]
    limit: Option<u32>,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<SubscriptionListArgs, _, _, _>(
        CommandSpec::from_args::<SubscriptionListArgs>(
            "list",
            "List available hosting plan subscriptions",
        )
        .with_long(
            "List hosting plan subscriptions available to attach to applications. \
             Results are autopaginated.",
        )
        .with_system("hosting")
        .with_tier(Tier::Read)
        .with_scopes(&[SUB_READ])
        .with_default_fields("totalAvailableSlots,items")
        .with_output_schema::<HostingSubscriptionList>(),
        |ctx, args: SubscriptionListArgs| async move {
            let limit = args.limit;
            let hosting_product = args.hosting_product.as_deref();
            let client = make_client(&ctx, &[SUB_READ]).await?;

            let mut all_items: Vec<Value> = Vec::new();
            let mut page_token: Option<String> = None;

            loop {
                let page_limit =
                    limit.map(|cap| cap.saturating_sub(all_items.len() as u32).min(100));

                let response = client
                    .list_subscriptions(page_token.as_deref(), page_limit, hosting_product)
                    .await
                    .map_err(client_err)?;

                if let Some(items) = response.get("items").and_then(|v| v.as_array()) {
                    all_items.extend(items.iter().cloned());
                }

                if limit.is_some_and(|cap| all_items.len() >= cap as usize) {
                    all_items.truncate(limit.expect("checked") as usize);
                    break;
                }

                match next_page_token(&response) {
                    Some(token) => page_token = Some(token),
                    None => break,
                }
            }

            let total_available_slots: u64 = all_items
                .iter()
                .filter_map(|item| item.get("availableSlots").and_then(|v| v.as_u64()))
                .sum();

            Ok(CommandResult::new(json!({
                "items": all_items,
                "totalAvailableSlots": total_available_slots,
            }))
            .with_next_actions(vec![
                next_action(
                    "hosting subscription attach --app-id <app-id> --subscription-id <id>",
                    "Attach an application to a hosting plan",
                )
                .with_param("app-id", NextActionParam::required())
                .with_param("subscription-id", NextActionParam::required()),
            ]))
        },
    )
}
