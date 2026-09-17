use crate::output_schema::output_schema;
use crate::shopping::SHOPPING_SCOPES;
use crate::shopping::common::{client_err, make_client, wait_duration, wait_for_order};
use crate::shopping::human::{ORDER_VIEW_ID, order_response};
use cli_engine::{CommandResult, CommandSpec, RuntimeCommandSpec, Tier};

output_schema!(OrderOutput {
    "ucp": "object";
    "id": "string";
    "checkout_id": "string";
    "line_items": "[]object";
    "totals": "[]object";
});

#[derive(Debug, Clone, clap::Args)]
struct Args {
    /// Completed order ID returned by checkout completion.
    #[arg(value_name = "ORDER_ID")]
    id: String,

    /// Poll until the new order becomes available.
    #[arg(long)]
    wait: bool,

    /// Maximum seconds to wait for order visibility (1-60, default 15).
    #[arg(long, value_name = "SECONDS", requires = "wait")]
    wait_timeout: Option<u8>,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<Args, _, _, _>(
        CommandSpec::from_args::<Args>("get", "Get a completed Shopping order")
            .with_long(
                "Read a completed order. New orders usually become available within 3-10 seconds; \
                 use --wait to poll for up to 15 seconds by default.",
            )
            .with_system("shopping")
            .with_tier(Tier::Read)
            .with_scopes(SHOPPING_SCOPES)
            .with_output_schema::<OrderOutput>()
            .with_view_id(ORDER_VIEW_ID),
        |ctx, args: Args| async move {
            let client = make_client(&ctx).await?;
            let order = if args.wait {
                let (order, _) = wait_for_order(
                    &client,
                    &args.id,
                    wait_duration(args.wait_timeout)?,
                    &ctx.middleware.env,
                )
                .await?;
                order
            } else {
                crate::shopping::client::get_order(&client, &args.id)
                    .await
                    .map_err(client_err)?
            };
            let order = serde_json::to_value(&order).map_err(|error| {
                crate::error::GddyError::unexpected(format!(
                    "failed to encode order response: {error}"
                ))
                .into_cli_error()
            })?;
            let output = if ctx.middleware.output_format == "human" {
                order_response(&order)
            } else {
                order
            };
            Ok(CommandResult::new(output))
        },
    )
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::shopping::human::order_response;

    #[test]
    fn human_response_shows_order_essentials_without_ucp_metadata() {
        let response = order_response(&json!({
            "id": "order-1",
            "checkout_id": "checkout-1",
            "permalink_url": "https://example.test/order-1",
            "currency": "USD",
            "line_items": [{"item": {"title": "Web Hosting Economy"}, "quantity": {"total": 1}, "status": "fulfilled"}],
            "totals": [{"type": "total", "display_text": "Total", "amount": 8388}],
            "ucp": {"do_not_render": true}
        }));

        assert_eq!(response["id"], "order-1");
        assert_eq!(response["line_items"][0]["status"], "fulfilled");
        assert_eq!(response["total"], "USD 83.88");
        assert!(!response.to_string().contains("do_not_render"));
    }

    #[test]
    fn human_response_omits_totals_without_a_currency() {
        let response = order_response(&json!({
            "id": "order-1",
            "totals": [{"display_text": "Total", "amount": 8388}]
        }));

        assert!(
            response
                .get("total")
                .is_some_and(serde_json::Value::is_null)
        );
        assert!(!response.to_string().contains("8388"));
    }
}
