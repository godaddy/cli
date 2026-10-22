//! `gddy domain transfer-lock` — lock or unlock a domain against transfers (v3).

use cli_engine::{
    CliCoreError, CommandResult, CommandSpec, GroupSpec, NextActionParam, RuntimeCommandSpec,
    RuntimeGroupSpec, Tier,
};
use serde_json::json;

use super::common::{
    api_error, format_operation_error, is_terminal_status, make_client, validate_domain_name,
};
use crate::next_action::next_action;
use crate::output_schema::output_schema;
use crate::scopes::DOMAINS_DOMAIN_UPDATE;

output_schema!(TransferLockResult {
    "domain": "string";
    // Matches Domain / `domain get` field name (`transferLock`), not a local alias.
    "transferLock": "boolean";
    // Present when the async operation returns them (serialized from Options).
    "operationId": "string", optional;
    "status": "string", optional;
});

/// Exactly one of `--enable` / `--disable`. Kept as its own derive struct so
/// the struct-level `#[group(...)]` only covers these two — `domain` stays
/// outside the group. See the flatten caveat on `CommandSpec::from_args`.
#[derive(Debug, Clone, clap::Args)]
#[group(required = true, multiple = false)]
struct TransferLockToggle {
    /// Lock the domain to prevent unauthorized transfers.
    #[arg(long)]
    enable: bool,

    /// Unlock the domain to allow transfers.
    #[arg(long)]
    disable: bool,
}

#[derive(Debug, Clone, clap::Args)]
struct TransferLockSetArgs {
    /// Domain to update (e.g. example.com).
    #[arg(value_name = "DOMAIN")]
    domain: String,

    #[command(flatten)]
    toggle: TransferLockToggle,
}

pub(super) fn group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(GroupSpec::new(
        "transfer-lock",
        "Manage a domain's transfer-lock setting",
    ))
    .with_command(RuntimeCommandSpec::new_typed_with_context::<
        TransferLockSetArgs,
        _,
        _,
        _,
    >(
        CommandSpec::from_args::<TransferLockSetArgs>(
            "set",
            "Lock or unlock a domain to prevent transfers",
        )
        .with_long(
            "Lock or unlock a domain's transfer-lock setting. When locked, the domain \
             cannot be transferred to another registrar without first unlocking it. \
             The change propagates to the registry asynchronously.\n\
             \n\
             AI assistants: Before using --disable, confirm the end user intends to \
             allow outbound transfer of this domain. Unlocking removes registrar-level \
             protection against unauthorized transfers.",
        )
        .with_system("domain")
        .with_tier(Tier::Mutate)
        .with_default_fields("domain,transferLock,operationId,status")
        .with_output_schema::<TransferLockResult>()
        .with_scopes(&[DOMAINS_DOMAIN_UPDATE]),
        |ctx, args: TransferLockSetArgs| async move {
            let domain = validate_domain_name(&args.domain)?;
            // Clap's ArgGroup guarantees exactly one of --enable/--disable was passed.
            let enabled = args.toggle.enable;
            let debug = !ctx.middleware.debug.is_empty();

            let client = make_client(&ctx).await?;
            let idempotency_key = uuid::Uuid::new_v4().to_string();
            let op = match client
                .set_transfer_lock()
                .domain_name(domain.as_str())
                .idempotency_key(idempotency_key)
                .body(enabled)
                .send()
                .await
            {
                Ok(r) => r.into_inner(),
                Err(e) => return Err(api_error("setting transfer lock", debug, e).await),
            };

            let status = op
                .status
                .as_ref()
                .map(|s| s.to_string())
                .unwrap_or_default();
            let operation_id = op.operation_id.as_ref().map(|id| id.to_string());

            if status == "FAILED" {
                let op_id = operation_id.as_deref().unwrap_or("unknown");
                let detail = format_operation_error(op.error.as_ref());
                return Err(CliCoreError::message(format!(
                    "transfer-lock update for {domain} failed (operation {op_id}){detail}"
                )));
            }

            // Only emit optional async-operation fields when present — never
            // JSON null (schema marks them optional strings; null leaks into tables).
            let mut result = json!({
                "domain": domain,
                "transferLock": enabled,
            });
            if let Some(op_id) = operation_id.as_ref() {
                result["operationId"] = json!(op_id);
            }
            if !status.is_empty() {
                result["status"] = json!(status);
            }

            let mut actions = Vec::new();
            if is_terminal_status(&status) {
                actions.push(
                    next_action(
                        "domain get <domain>",
                        "Confirm the updated transfer-lock setting",
                    )
                    .with_param("domain", NextActionParam::value(domain)),
                );
            } else if let Some(op_id) = operation_id.as_ref() {
                actions.push(
                    next_action(
                        "domain operation status <operation-id>",
                        "Check whether the transfer-lock update has finished",
                    )
                    .with_param("operation-id", NextActionParam::value(op_id)),
                );
            }

            Ok(CommandResult::new(result).with_next_actions(actions))
        },
    ))
}
