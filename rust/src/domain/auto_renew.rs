//! `gddy domain auto-renew` — toggle auto-renewal for a domain (v3).

use cli_engine::{
    CliCoreError, CommandResult, CommandSpec, GroupSpec, NextActionParam, RuntimeCommandSpec,
    RuntimeGroupSpec, Tier,
};
use serde_json::json;

use super::common::{
    api_error, format_operation_error, is_terminal_status, make_client, validate_domain_name,
};
use crate::idn::DomainFormArgs;
use crate::next_action::next_action;
use crate::output_schema::output_schema;
use crate::scopes::DOMAINS_DOMAIN_UPDATE;

output_schema!(AutoRenewResult {
    "domain": "string";
    "autoRenew": "boolean";
    // Present when the async operation returns them (serialized from Options).
    "operationId": "string", optional;
    "status": "string", optional;
});

/// Exactly one of `--enable` / `--disable`. Kept as its own derive struct so
/// the struct-level `#[group(...)]` only covers these two — `domain` stays
/// outside the group. See the flatten caveat on `CommandSpec::from_args`.
#[derive(Debug, Clone, clap::Args)]
#[group(required = true, multiple = false)]
struct AutoRenewToggle {
    /// Turn on auto-renewal before expiration.
    #[arg(long)]
    enable: bool,

    /// Turn off auto-renewal.
    #[arg(long)]
    disable: bool,
}

#[derive(Debug, Clone, clap::Args)]
struct AutoRenewSetArgs {
    /// Domain to update (e.g. example.com).
    #[arg(value_name = "DOMAIN")]
    domain: String,

    #[command(flatten)]
    domain_format: DomainFormArgs,

    #[command(flatten)]
    toggle: AutoRenewToggle,
}

pub(super) fn group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(GroupSpec::new(
        "auto-renew",
        "Manage a domain's auto-renewal setting",
    ))
    .with_command(RuntimeCommandSpec::new_typed_with_context::<
        AutoRenewSetArgs,
        _,
        _,
        _,
    >(
        CommandSpec::from_args::<AutoRenewSetArgs>(
            "set",
            "Enable or disable auto-renewal for a domain",
        )
        .with_long(
            "Turn auto-renewal on or off for a domain you own. When enabled, the \
             domain renews automatically before expiration using the payment method \
             on file. The change runs asynchronously — poll with `domain operation \
             status` until it finishes.",
        )
        .with_system("domain")
        .with_tier(Tier::Mutate)
        .with_default_fields("domain,autoRenew,operationId,status")
        .with_output_schema::<AutoRenewResult>()
        .with_scopes(&[DOMAINS_DOMAIN_UPDATE]),
        |ctx, args: AutoRenewSetArgs| async move {
            let domain = validate_domain_name(&args.domain)?;
            // Clap's ArgGroup guarantees exactly one of --enable/--disable was passed.
            let enabled = args.toggle.enable;
            let debug = !ctx.middleware.debug.is_empty();
            // Resolve the display form before the update is sent, so a bad
            // preference can't hide the operation ID of a change already made.
            let domain_format = args.domain_format.form(&ctx)?;

            let client = make_client(&ctx).await?;
            let idempotency_key = uuid::Uuid::new_v4().to_string();
            let op = match client
                .set_auto_renew()
                .domain_name(domain.as_str())
                .idempotency_key(idempotency_key)
                .body(enabled)
                .send()
                .await
            {
                Ok(r) => r.into_inner(),
                Err(e) => return Err(api_error("setting auto-renew", debug, e).await),
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
                    "auto-renew update for {domain} failed (operation {op_id}){detail}"
                )));
            }

            // Only emit optional async-operation fields when present — never
            // JSON null (schema marks them optional strings; null leaks into tables).
            let mut result = json!({
                "domain": domain,
                "autoRenew": enabled,
            });
            if let Some(op_id) = operation_id.as_ref() {
                result["operationId"] = json!(op_id);
            }
            if !status.is_empty() {
                result["status"] = json!(status);
            }

            domain_format.convert_fields(&mut result, &["domain"]);

            let mut actions = Vec::new();
            if is_terminal_status(&status) {
                actions.push(
                    next_action(
                        "domain get <domain>",
                        "Confirm the updated auto-renew setting",
                    )
                    .with_param(
                        "domain",
                        NextActionParam::value(domain_format.render(&domain)),
                    ),
                );
            } else if let Some(op_id) = operation_id.as_ref() {
                actions.push(
                    next_action(
                        "domain operation status <operation-id>",
                        "Check whether the auto-renew update has finished",
                    )
                    .with_param("operation-id", NextActionParam::value(op_id)),
                );
            }

            Ok(CommandResult::new(result).with_next_actions(actions))
        },
    ))
}
