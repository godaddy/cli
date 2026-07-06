//! `gddy domain nameservers` — manage a domain's nameservers (v3).

use cli_engine::{
    CommandResult, CommandSpec, GroupSpec, RuntimeCommandSpec, RuntimeGroupSpec, Tier,
};
use serde_json::json;

use domains_client::types;

use super::common::{api_error, make_client, string_list};
use crate::output_schema::output_schema;
use crate::scopes::DOMAINS_NAMESERVER_UPDATE;

output_schema!(NameserversResult {
    "domain": "string";
    "nameservers": "[]string";
    // Present when the async operation returns them (serialized from Options).
    "operationId": "string", optional;
    "status": "string", optional;
});

pub(super) fn group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(GroupSpec::new(
        "nameservers",
        "Manage a domain's nameservers",
    ))
    .with_command(RuntimeCommandSpec::new_with_context(
        CommandSpec::new("set", "Replace a domain's nameservers")
            .with_long(
                "Replace the full set of nameservers for a domain you own. Pass \
                 --nameserver once per host. This is destructive — it overwrites the \
                 existing nameservers — and runs asynchronously.",
            )
            .with_system("domain")
            .with_tier(Tier::Destructive)
            .with_default_fields("domain,nameservers,operationId,status")
            .with_output_schema::<NameserversResult>()
            .with_scopes(&[DOMAINS_NAMESERVER_UPDATE])
            .with_arg(
                clap::Arg::new("domain")
                    .value_name("DOMAIN")
                    .required(true)
                    .help("Domain whose nameservers to replace (e.g. example.com)"),
            )
            .with_arg(
                clap::Arg::new("nameserver")
                    .long("nameserver")
                    .value_name("HOST")
                    .required(true)
                    .action(clap::ArgAction::Append)
                    .help("Nameserver host (repeatable; replaces the full set)"),
            ),
        |ctx| async move {
            let domain = ctx
                .args
                .get("domain")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_owned();
            let hosts = string_list(&ctx, "nameserver");
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
            Ok(CommandResult::new(result))
        },
    ))
}
