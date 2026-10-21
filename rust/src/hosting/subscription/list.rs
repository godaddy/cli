use cli_engine::{
    Alignment, CommandResult, CommandSpec, NextAction, NextActionParam, RuntimeCommandSpec,
    TableColumn, Tier,
};
use serde_json::{Value, json};

use crate::hosting::client::{ClientError, HostingClient};
use crate::hosting::common::{HostingProduct, HostingSubscriptionList, client_err, make_client};
use crate::next_action::next_action;
use crate::scopes::HOSTING_SUBSCRIPTION_READ as SUB_READ;

#[derive(Debug, Clone, clap::Args)]
struct SubscriptionListArgs {
    /// Hosting product to list (WEB_HOSTING or MANAGED_WORDPRESS).
    #[arg(long, value_name = "PRODUCT", ignore_case = true)]
    hosting_product: HostingProduct,

    /// Maximum number of subscriptions to return. Omit to return all.
    #[arg(long, value_name = "N", value_parser = clap::value_parser!(u32).range(1..))]
    limit: Option<u32>,
}

fn view_columns() -> Vec<TableColumn> {
    vec![
        TableColumn::new("totalAvailableSlots", "Available Slots").align(Alignment::Right),
        TableColumn::new("items", "Plans").nested(vec![
            TableColumn::new("subscriptionId", "Subscription ID").essential(true),
            TableColumn::new("label", "Plan"),
            TableColumn::new("tier", "Tier"),
            TableColumn::new("status", "Status"),
            TableColumn::new("availableSlots", "Free").align(Alignment::Right),
            TableColumn::new("slotLimit", "Slots").align(Alignment::Right),
            TableColumn::new("paidThroughDate", "Paid Through"),
        ]),
    ]
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<SubscriptionListArgs, _, _, _>(
        CommandSpec::from_args::<SubscriptionListArgs>(
            "list",
            "List available hosting plan subscriptions",
        )
        .with_long(
            "List hosting plan subscriptions available to attach to applications. \
             --hosting-product is required.",
        )
        .with_system("hosting")
        .with_tier(Tier::Read)
        .with_scopes(&[SUB_READ])
        .with_default_fields("totalAvailableSlots,items")
        .with_output_schema::<HostingSubscriptionList>()
        .with_view(view_columns()),
        |ctx, args: SubscriptionListArgs| async move {
            let client = make_client(&ctx, &[SUB_READ]).await?;
            let items = fetch_subscriptions(&client, args.hosting_product.as_str(), args.limit)
                .await
                .map_err(client_err)?;

            let total_available_slots: u64 = items
                .iter()
                .filter_map(|item| item.get("availableSlots").and_then(|v| v.as_u64()))
                .sum();

            let next_actions = list_next_actions(&items);
            Ok(CommandResult::new(json!({
                "items": items,
                "totalAvailableSlots": total_available_slots,
            }))
            .with_next_actions(next_actions))
        },
    )
}

/// The endpoint takes no paging parameters yet, so this is a single request
/// even if the response carries a `rel=next` link.
async fn fetch_subscriptions(
    client: &HostingClient,
    hosting_product: &str,
    limit: Option<u32>,
) -> Result<Vec<Value>, ClientError> {
    let response = client.list_subscriptions(hosting_product).await?;
    let mut items = response
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if let Some(cap) = limit {
        items.truncate(cap as usize);
    }
    Ok(items)
}

fn list_next_actions(items: &[Value]) -> Vec<NextAction> {
    let attachable: Vec<String> = items
        .iter()
        .filter(|item| {
            item.get("availableSlots")
                .and_then(|v| v.as_u64())
                .unwrap_or(0)
                > 0
        })
        .filter_map(|item| {
            item.get("subscriptionId")
                .and_then(|v| v.as_str())
                .map(str::to_owned)
        })
        .collect();

    if attachable.is_empty() {
        return vec![next_action(
            "shopping catalog search --category webHosting",
            "Buy a Web Hosting plan (NODEJS). Other app types will use a different category.",
        )];
    }

    let mut subscription_id = NextActionParam::required();
    subscription_id.r#enum = attachable;
    subscription_id.description = Some(
        "Hosting plan with availableSlots > 0. Confirm which plan to attach. \
         An app is attached to one plan."
            .to_owned(),
    );

    vec![
        next_action(
            "hosting subscription attach --app-id <app-id> --subscription-id <id>",
            "Attach an application to a hosting plan",
        )
        .with_param("app-id", NextActionParam::required())
        .with_param("subscription-id", subscription_id),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fetch_makes_one_request_even_with_a_next_link() {
        let server = httpmock::MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::GET)
                    .path("/v1/hosting/subscriptions")
                    .query_param("hostingProduct", "WEB_HOSTING");
                then.status(200).json_body(json!({
                    "items": [
                        { "subscriptionId": "sub-1", "availableSlots": 1 },
                        { "subscriptionId": "sub-2", "availableSlots": 0 }
                    ],
                    "links": [{
                        "rel": "next",
                        "href": "https://api.godaddy.com/v1/hosting/subscriptions?hostingProduct=WEB_HOSTING&pageToken=tok-2"
                    }]
                }));
            })
            .await;

        let client = HostingClient::new(server.base_url(), "test-token");
        let items = fetch_subscriptions(&client, "WEB_HOSTING", Some(1))
            .await
            .expect("fetch subscriptions");

        mock.assert_calls_async(1).await;
        assert_eq!(
            items,
            vec![json!({ "subscriptionId": "sub-1", "availableSlots": 1 })]
        );
    }

    #[test]
    fn plans_render_as_a_nested_table() {
        let rendered = cli_engine::preview_human_view(
            json!({
                "totalAvailableSlots": 2,
                "items": [{
                    "subscriptionId": "sub-1",
                    "hostingProduct": "WEB_HOSTING",
                    "label": "Web Hosting Deluxe",
                    "tier": "DELUXE",
                    "status": "ACTIVE",
                    "attachedCount": 1,
                    "slotLimit": 3,
                    "availableSlots": 2,
                    "paidThroughDate": "2027-08-06T00:00:00Z",
                    "links": []
                }]
            }),
            &view_columns(),
        );
        assert!(rendered.contains("Plans:"), "{rendered}");
        assert!(rendered.contains("SUBSCRIPTION ID"), "{rendered}");
        assert!(rendered.contains("sub-1"), "{rendered}");
        assert!(!rendered.contains('{'), "{rendered}");
    }

    #[test]
    fn list_next_actions_attach_when_a_slot_is_free() {
        let actions = list_next_actions(&[json!({
            "subscriptionId": "sub-1",
            "availableSlots": 2
        })]);
        assert_eq!(actions.len(), 1);
        assert!(actions[0].command.contains("subscription attach"));
        assert_eq!(
            actions[0].params["subscription-id"].r#enum,
            vec!["sub-1".to_owned()]
        );
    }

    #[test]
    fn list_next_actions_shop_when_no_slots() {
        let actions = list_next_actions(&[json!({
            "subscriptionId": "sub-full",
            "availableSlots": 0
        })]);
        assert_eq!(actions.len(), 1);
        assert_eq!(
            actions[0].command,
            "gddy shopping catalog search --category webHosting"
        );
    }

    #[test]
    fn list_next_actions_shop_when_list_is_empty() {
        let actions = list_next_actions(&[]);
        assert_eq!(
            actions[0].command,
            "gddy shopping catalog search --category webHosting"
        );
    }
}
