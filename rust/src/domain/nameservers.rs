//! `gddy domain nameservers` — manage a domain's nameservers (v3).

use cli_engine::{
    CommandResult, CommandSpec, GroupSpec, NextActionParam, RuntimeCommandSpec, RuntimeGroupSpec,
    Tier,
};
use serde_json::json;

use domains_client::types;

use super::common::{api_error, make_client, validate_domain_name, validate_nameserver_hosts};
use crate::next_action::next_action;
use crate::output_schema::output_schema;
use crate::scopes::DOMAINS_NAMESERVER_UPDATE;

output_schema!(NameserversResult {
    "domain": "string";
    "nameservers": "[]string";
    // Present when the async operation returns them (serialized from Options).
    "operationId": "string", optional;
    "status": "string", optional;
});

#[derive(Debug, Clone, clap::Args)]
struct NameserversSetArgs {
    /// Domain whose nameservers to replace (e.g. example.com).
    #[arg(value_name = "DOMAIN")]
    domain: String,

    /// Nameserver host (repeatable; replaces the full set).
    #[arg(long, value_name = "HOST", required = true)]
    nameserver: Vec<String>,
}

pub(super) fn group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(GroupSpec::new(
        "nameservers",
        "Manage a domain's nameservers",
    ))
    .with_command(RuntimeCommandSpec::new_typed_with_context::<
        NameserversSetArgs,
        _,
        _,
        _,
    >(
        CommandSpec::from_args::<NameserversSetArgs>("set", "Replace a domain's nameservers")
            .with_long(
                "Replace the full set of nameservers for a domain you own. Pass \
                 --nameserver once per host. This is destructive — it overwrites the \
                 existing nameservers — and runs asynchronously.",
            )
            .with_system("domain")
            .with_tier(Tier::Destructive)
            .with_default_fields("domain,nameservers,operationId,status")
            .with_output_schema::<NameserversResult>()
            .with_scopes(&[DOMAINS_NAMESERVER_UPDATE]),
        |ctx, args: NameserversSetArgs| async move {
            let domain = validate_domain_name(&args.domain)?;
            let hosts = validate_nameserver_hosts(args.nameserver)?;
            let debug = !ctx.middleware.debug.is_empty();

            let client = make_client(&ctx).await?;
            let body = types::NameServers(
                hosts
                    .iter()
                    .map(|h| types::NameserverHostname(h.clone()))
                    .collect(),
            );
            let idempotency_key = uuid::Uuid::new_v4().to_string();
            let op = match client
                .update_nameservers()
                .domain_name(domain.as_str())
                .idempotency_key(idempotency_key)
                .body(body)
                .send()
                .await
            {
                Ok(r) => r.into_inner(),
                Err(e) => return Err(api_error("updating nameservers", debug, e).await),
            };
            // Only emit the optional async-operation fields when present — never
            // JSON null (schema marks them optional strings; null leaks into tables).
            let mut result = json!({
                "domain": domain,
                "nameservers": hosts,
            });
            if let Some(op_id) = op.operation_id.as_ref() {
                result["operationId"] = json!(op_id.to_string());
            }
            if let Some(status) = op.status.as_ref() {
                result["status"] = json!(status.to_string());
            }
            Ok(CommandResult::new(result).with_next_actions(vec![
                next_action("domain get <domain>", "Confirm the updated nameservers")
                    .with_param("domain", NextActionParam::value(domain)),
            ]))
        },
    ))
}
