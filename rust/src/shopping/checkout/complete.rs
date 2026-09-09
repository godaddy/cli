use cli_engine::{CommandResult, CommandSpec, RuntimeCommandSpec, Tier};
use serde_json::json;

use crate::next_action::next_action;
use crate::shopping::SHOPPING_SCOPES;
use crate::shopping::common::{
    client_err, has_conflicting_checkout_id, make_client, read_json, require_idempotency_key,
    wait_duration, wait_for_order,
};

#[derive(Debug, Clone, clap::Args)]
struct Args {
    /// Checkout session ID.
    #[arg(value_name = "CHECKOUT_ID")]
    id: String,

    /// Completion request as raw JSON. It must include idempotency_key.
    #[arg(long, value_name = "JSON", required_unless_present = "file")]
    body: Option<String>,

    /// Path to a JSON completion request. Takes precedence over --body.
    #[arg(long, value_name = "PATH")]
    file: Option<String>,

    /// Wait for the completed order to become available.
    #[arg(long)]
    wait_for_order: bool,

    /// Maximum seconds to wait for order visibility (1-60, default 15).
    #[arg(long, value_name = "SECONDS", requires = "wait_for_order")]
    timeout: Option<String>,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<Args, _, _, _>(
        CommandSpec::from_args::<Args>("complete", "Complete a Shopping checkout and place an order")
            .with_long(
                "Places a real order. Supply the Shopping API completion request through --body or \
                 --file; it must include a selected saved payment instrument and a non-empty \
                 idempotency_key. The CLI never retries completion automatically. Use --wait-for-order \
                 to poll until the new order becomes available after success.",
            )
            .with_system("shopping")
            .with_tier(Tier::Mutate)
            .mutates(true)
            .handles_dry_run(true)
            .with_scopes(SHOPPING_SCOPES)
            .auth_optional(),
        |ctx, args: Args| async move {
            let body = read_json(args.body.as_deref(), args.file.as_deref(), "object")?;
            require_idempotency_key(&body)?;
            if has_conflicting_checkout_id(&body, &args.id) {
                return Err(crate::error::GddyError::validation(
                    "checkout ID in request body conflicts with CHECKOUT_ID",
                )
                .into_cli_error());
            }
            if ctx.dry_run() {
                return Ok(CommandResult::new(json!({
                    "action": "dry-run: would complete checkout",
                    "id": args.id,
                    "body": body,
                })));
            }

            let client = make_client(&ctx).await?;
            let completion = client.complete_checkout(&args.id, body).await.map_err(client_err)?;
            let order_id = completion
                .pointer("/order/id")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned);

            if args.wait_for_order {
                let order_id = order_id.ok_or_else(|| {
                    crate::error::GddyError::unexpected(
                        "completion response did not include order.id for --wait-for-order",
                    )
                    .into_cli_error()
                })?;
                let timeout = wait_duration(args.timeout.as_deref())?;
                let (order, attempts) = wait_for_order(&client, &order_id, timeout).await?;
                return Ok(CommandResult::new(json!({
                    "checkout": completion,
                    "order": order,
                    "order_read_attempts": attempts,
                })));
            }

            let mut result = CommandResult::new(completion);
            if let Some(order_id) = order_id {
                result = result.with_next_actions(vec![next_action(
                    format!("shopping order get {order_id} --wait"),
                    "Read the completed order after it becomes visible",
                )]);
            }
            Ok(result)
        },
    )
}
