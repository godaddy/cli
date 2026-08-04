//! `gddy domain operation status` — poll an async domain operation (v3).
//!
//! Every async domain mutation (register, nameserver updates, and eventually
//! renew/transfer) returns an `operationId` that can be re-checked here via the
//! same `GET /v3/domains/operations/{operationId}` endpoint `purchase`'s
//! bounded poll loop already uses internally.

use cli_engine::{
    CommandResult, CommandSpec, GroupSpec, NextActionParam, RuntimeCommandSpec, RuntimeGroupSpec,
    Tier,
};
use serde_json::json;

use domains_client::types;

use super::common::{api_error, format_operation_error, is_terminal_status, make_client};
use crate::next_action::next_action;
use crate::output_schema::output_schema;
use crate::scopes::DOMAINS_READ;

output_schema!(DomainOperationStatusResult {
    "operationId": "string";
    "type": "string";
    "domain": "string";
    "status": "string";
    // Only populated once the operation reaches a terminal state.
    "orderId": "string", optional;
    "expiresAt": "string", optional;
    "error": "string", optional;
});

#[derive(Debug, Clone, clap::Args)]
struct OperationStatusArgs {
    /// The operationId returned by an async domain mutation.
    #[arg(value_name = "OPERATION_ID")]
    operation_id: String,
}

fn status_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<OperationStatusArgs, _, _, _>(
        CommandSpec::from_args::<OperationStatusArgs>(
            "status",
            "Check the status of an async domain operation",
        )
        .with_long(
            "Check on a domain operation (registration, nameserver update, and \
                 similar async mutations) by the `operationId` it returned. This is a \
                 read-only check: even a FAILED operation is reported as data with exit \
                 code 0, since the status check itself succeeded.",
        )
        .with_system("domain")
        .with_tier(Tier::Read)
        .with_default_fields("operationId,type,domain,status,orderId,expiresAt,error")
        .with_output_schema::<DomainOperationStatusResult>()
        .with_scopes(&[DOMAINS_READ]),
        |ctx, args: OperationStatusArgs| async move {
            let operation_id = args.operation_id;
            let debug = !ctx.middleware.debug.is_empty();

            let client = make_client(&ctx).await?;
            let op = match client
                .get_operation()
                .operation_id(types::Uuid(operation_id.clone()))
                .send()
                .await
            {
                Ok(r) => r.into_inner(),
                Err(e) => return Err(api_error("checking domain operation status", debug, e).await),
            };

            let status = op
                .status
                .as_ref()
                .map(|s| s.to_string())
                .unwrap_or_default();
            let domain = op.domain.clone().unwrap_or_default();
            let op_id = op
                .operation_id
                .as_ref()
                .map(|id| id.to_string())
                .unwrap_or(operation_id);

            let op_type = op.type_.as_ref().map(|t| t.to_string()).unwrap_or_default();
            let mut result = json!({
                "operationId": op_id,
                "type": op_type,
                "domain": domain,
                "status": status,
            });
            if let Some(op_result) = op.result.as_ref() {
                if let Some(order_id) = op_result.order_id.as_ref() {
                    result["orderId"] = json!(order_id);
                }
                if let Some(expires_at) = op_result.expires_at.as_ref() {
                    result["expiresAt"] = json!(expires_at.to_string());
                }
            }
            if let Some(error) = op.error.as_ref() {
                // `format_operation_error` wraps its result in a leading space and
                // parens for inline sentence-appending (as `purchase.rs` uses it);
                // strip that decoration since this is a standalone field value.
                let detail = format_operation_error(Some(error));
                let detail = detail
                    .strip_prefix(" (")
                    .and_then(|s| s.strip_suffix(')'))
                    .unwrap_or(&detail);
                if !detail.is_empty() {
                    result["error"] = json!(detail);
                }
            }

            let mut actions = Vec::new();
            if is_terminal_status(&status) {
                if status == "COMPLETED" && !domain.is_empty() {
                    actions.push(
                        next_action("domain get <domain>", "See the domain's details")
                            .with_param("domain", NextActionParam::value(domain)),
                    );
                }
            } else {
                actions.push(
                    next_action(
                        "domain operation status <operation-id>",
                        "Re-check whether the operation has finished",
                    )
                    .with_param("operation-id", NextActionParam::value(op_id)),
                );
            }

            Ok(CommandResult::new(result).with_next_actions(actions))
        },
    )
}

pub(super) fn group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(GroupSpec::new(
        "operation",
        "Check the status of async domain operations",
    ))
    .with_command(status_command())
}
