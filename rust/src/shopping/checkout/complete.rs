use cli_engine::{CommandResult, CommandSpec, ModuleContext, RuntimeCommandSpec, Tier};
use serde_json::{Value, json};

use crate::next_action::{next_action, required_value};
use crate::shopping::client::ClientError;
use crate::shopping::common::{
    ensure_completion_idempotency_key, has_conflicting_checkout_id, make_client, read_json,
    require_selected_payment_instrument,
};
use crate::shopping::{SHOPPING_SCOPES, command_for_env};

#[derive(Debug, Clone, clap::Args)]
struct Args {
    /// Checkout session ID.
    #[arg(value_name = "CHECKOUT_ID")]
    id: String,

    /// Completion request as raw JSON. Omit idempotency_key to let gddy generate one.
    #[arg(long, value_name = "JSON", required_unless_present = "file")]
    body: Option<String>,

    /// Path to a JSON completion request. Takes precedence over --body.
    #[arg(long, value_name = "PATH")]
    file: Option<String>,
}

const HUMAN_VIEW_ID: &str = "shopping-checkout-complete";

pub(crate) fn register_human_view(ctx: &mut ModuleContext<'_>) {
    ctx.middleware_mut()
        .human_views
        .register_func(HUMAN_VIEW_ID, render_human);
}

fn human_response(completion: &Value, idempotency_key: &str) -> Value {
    json!({
        "checkout_id": completion.get("id").and_then(Value::as_str).unwrap_or_default(),
        "status": completion.get("status").and_then(Value::as_str).unwrap_or_default(),
        "order_id": completion.pointer("/order/id").and_then(Value::as_str),
        "idempotency_key": idempotency_key,
    })
}

fn render_human(completion: &Value) -> String {
    if let Some(action) = completion.get("action").and_then(Value::as_str) {
        let idempotency_key = completion
            .get("idempotency_key")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let body = completion.get("body").cloned().unwrap_or(Value::Null);
        return format!(
            "{}\nCheckout: {}\nIdempotency key: {idempotency_key}\nRequest:\n{}\n",
            action,
            completion
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            serde_json::to_string_pretty(&body).unwrap_or_else(|_| body.to_string()),
        );
    }
    let mut output = format!(
        "Checkout: {}\nStatus: {}\nIdempotency key: {}\n",
        completion
            .get("checkout_id")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        completion
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        completion
            .get("idempotency_key")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    );
    if let Some(order_id) = completion.get("order_id").and_then(Value::as_str) {
        output.push_str(&format!("Order: {order_id}\n"));
    }
    output.push_str("\nKeep this idempotency key. Do not retry a completion unless you first confirm its outcome.\n");
    output
}

fn completion_error(error: ClientError, idempotency_key: &str) -> cli_engine::CliCoreError {
    crate::error::GddyError::from(error)
        .with_fix(format!(
            "Completion may have reached Shopping. Do not retry automatically. Reuse idempotency_key \
             {idempotency_key:?} only for the same intended purchase after confirming its outcome."
        ))
        .into_cli_error()
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<Args, _, _, _>(
        CommandSpec::from_args::<Args>("complete", "Complete a Shopping checkout and place an order")
            .with_long(
                "Places a real order. Supply the Shopping API completion request through --body or \
                 --file; it must include a selected saved payment instrument. Supply a non-empty \
                 idempotency_key to control retries, or omit it to let gddy generate and return one. \
                 The CLI never retries completion automatically. Read the resulting order with \
                 `shopping order get <order-id> --wait`.",
            )
            .with_system("shopping")
            .with_tier(Tier::Mutate)
            .mutates(true)
            .handles_dry_run(true)
            .with_scopes(SHOPPING_SCOPES)
            .auth_optional()
            .with_view_id(HUMAN_VIEW_ID),
        |ctx, args: Args| async move {
            let mut body = read_json(args.body.as_deref(), args.file.as_deref(), "object")?;
            if has_conflicting_checkout_id(&body, &args.id) {
                return Err(crate::error::GddyError::validation(
                    "checkout ID in request body conflicts with CHECKOUT_ID",
                )
                .into_cli_error());
            }
            require_selected_payment_instrument(&body)?;
            let idempotency_key = ensure_completion_idempotency_key(&mut body)?;
            if ctx.dry_run() {
                return Ok(CommandResult::new(json!({
                    "action": "dry-run: would complete checkout",
                    "id": args.id,
                    "idempotency_key": idempotency_key,
                    "body": body,
                })));
            }

            let client = make_client(&ctx).await?;
            let completion = client.complete_checkout(&args.id, body).await.map_err(|error| {
                completion_error(error, &idempotency_key)
            })?;
            let order_id = completion
                .pointer("/order/id")
                .and_then(Value::as_str)
                .map(str::to_owned);
            let mut result = CommandResult::new(if ctx.middleware.output_format == "human" {
                human_response(&completion, &idempotency_key)
            } else {
                completion
            });
            if let Some(order_id) = order_id {
                result = result.with_next_actions(vec![
                    next_action(
                        command_for_env(
                            &ctx.middleware.env,
                            format!("order get {order_id} --wait"),
                        ),
                        "Read the completed order after it becomes visible",
                    )
                    .with_param("order_id", required_value(order_id))
                    .with_param("idempotency_key", required_value(idempotency_key)),
                ]);
            }
            Ok(result)
        },
    )
}
