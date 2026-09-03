//! `gddy domain get` — show full details for one owned domain (v3).

use cli_engine::{
    CliCoreError, CommandResult, CommandSpec, NextActionParam, RuntimeCommandSpec, Tier,
};

use domains_client::types;

use super::common::{fetch_with_domain_retry, resolve_domain_name};
use crate::next_action::next_action;
use crate::scopes::DOMAINS_READ;

const DOMAIN_PROMPT: &str = "Domain to look up (e.g. example.com)";

#[derive(Debug, Clone, clap::Args)]
struct GetArgs {
    /// Domain to look up (must be in your account), e.g. example.com.
    #[arg(value_name = "DOMAIN")]
    domain: String,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<GetArgs, _, _, _>(
        CommandSpec::from_args::<GetArgs>("get", "Show full details for one of your domains")
            .with_long(
                "Show every detail for a single domain in your account (status, \
                 expiry, nameservers, and more). Unlike `list`, this shows all fields \
                 by default. The domain must be one you own.",
            )
            .with_system("domain")
            .with_tier(Tier::Read)
            .with_json_schema::<types::Domain>()
            .with_scopes(&[DOMAINS_READ]),
        |ctx, args: GetArgs| async move {
            let debug = !ctx.middleware.debug.is_empty();
            let domain = resolve_domain_name(&ctx, &args.domain, DOMAIN_PROMPT)?;
            let detail = fetch_with_domain_retry(
                &ctx,
                domain,
                DOMAIN_PROMPT,
                "retrieving domain details",
                debug,
                |client, domain| async move {
                    client
                        .get_domain()
                        .domain_name(domain.as_str())
                        .send()
                        .await
                        .map(|r| r.into_inner())
                },
            )
            .await?;
            let value = serde_json::to_value(&detail).map_err(|e| {
                CliCoreError::message(format!("failed to serialize domain details: {e}"))
            })?;
            Ok(CommandResult::new(value).with_next_actions(vec![
                next_action("dns list <domain>", "View this domain's DNS records")
                    .with_param("domain", NextActionParam::required()),
            ]))
        },
    )
}
