//! `gddy domain get` — show full details for one owned domain (v3).

use cli_engine::{
    CliCoreError, CommandResult, CommandSpec, NextAction, NextActionParam, RuntimeCommandSpec, Tier,
};

use domains_client::types;

use super::common::{api_error, make_client};
use crate::scopes::DOMAINS_READ;

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_with_context(
        CommandSpec::new("get", "Show full details for one of your domains")
            .with_long(
                "Show every detail for a single domain in your account (status, \
                 expiry, nameservers, and more). Unlike `list`, this shows all fields \
                 by default. The domain must be one you own.",
            )
            .with_system("domain")
            .with_tier(Tier::Read)
            .with_json_schema::<types::Domain>()
            .with_scopes(&[DOMAINS_READ])
            .with_arg(
                clap::Arg::new("domain")
                    .value_name("DOMAIN")
                    .required(true)
                    .help("Domain to look up (must be in your account), e.g. example.com"),
            ),
        |ctx| async move {
            let domain = ctx
                .args
                .get("domain")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_owned();
            let debug = !ctx.middleware.debug.is_empty();
            let client = make_client(&ctx).await?;
            let detail = match client
                .get_domain()
                .domain_name(domain.as_str())
                .send()
                .await
            {
                Ok(r) => r.into_inner(),
                Err(e) => return Err(api_error("retrieving domain details", debug, e).await),
            };
            let value = serde_json::to_value(&detail).map_err(|e| {
                CliCoreError::message(format!("failed to serialize domain details: {e}"))
            })?;
            Ok(CommandResult::new(value).with_next_actions(vec![
                NextAction::new("dns list <domain>", "View this domain's DNS records")
                    .with_param("domain", NextActionParam::required()),
            ]))
        },
    )
}
