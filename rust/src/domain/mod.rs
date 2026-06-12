//! `gddy domain` — domain availability and suggestions.
//!
//! These endpoints (the public GoDaddy Domains API) authenticate with an
//! sso-key, not OAuth. The HTTP layer is the typed, spec-generated
//! [`domains_client`] crate; auth scheme selection (sso-key vs Bearer) is driven
//! by the credential the [`CompositeAuthProvider`](crate::auth::CompositeAuthProvider)
//! returns for `domain:*` commands.

use cli_engine::{
    CliCoreError, CommandContext, CommandResult, CommandSpec, GroupSpec, Module, NextAction,
    NextActionParam, Result, RuntimeCommandSpec, RuntimeGroupSpec, Tier,
};
use serde_json::json;

use crate::{auth::SSO_KEY_PROVIDER, environments};

const USER_AGENT: &str = concat!("godaddy-cli/", env!("CARGO_PKG_VERSION"));

/// OAuth scope the domain availability + suggest endpoints require.
///
/// Source of truth (undocumented in the published Swagger spec): the GoDaddy
/// domains OAuth scope → endpoint whitelist in `gdcorp-domains/api-domain-data`,
/// `api/oauthscopewhitelist.json`. Both `GET /v1/domains/available` and
/// `GET /v1/domains/suggest` are listed under `domains.domain:read`:
/// <https://github.com/gdcorp-domains/api-domain-data/blob/main/api/oauthscopewhitelist.json>
///
/// Declared on the commands via [`CommandSpec::with_scopes`] so cli-engine's
/// OAuth scope step-up mints a token carrying it. Ignored on the sso-key path
/// (sso-key auth is unscoped).
const DOMAINS_READ_SCOPE: &str = "domains.domain:read";

fn map_env_err(e: environments::EnvError) -> CliCoreError {
    CliCoreError::message(e.to_string())
}

/// Convert a currency micro-unit amount (e.g. 11_990_000) to a decimal string
/// ("11.99"). Domain prices are returned in micro-units.
fn format_price(micros: Option<i64>) -> Option<String> {
    micros.map(|m| format!("{}.{:02}", m / 1_000_000, (m.abs() % 1_000_000) / 10_000))
}

/// Build a Domains API client for the active environment, choosing the auth
/// scheme from the resolved credential (sso-key for the bypass path, else
/// Bearer). The credential is resolved through the registered composite provider.
async fn make_client(ctx: &CommandContext) -> Result<domains_client::Client> {
    let env = ctx.middleware.env.clone();
    let domains = environments::resolve_domains(&env).map_err(map_env_err)?;
    let cred = ctx.credential().await?;
    let authorization = if cred.provider == SSO_KEY_PROVIDER {
        format!("sso-key {}", cred.token)
    } else {
        format!("Bearer {}", cred.token)
    };
    let request_id = uuid::Uuid::new_v4().to_string();
    domains_client::client_with_auth(&domains.base_url, &authorization, USER_AGENT, &request_id)
        .map_err(|e| CliCoreError::message(format!("failed to build domains client: {e}")))
}

fn string_list(ctx: &CommandContext, key: &str) -> Vec<String> {
    match ctx.args.get(key) {
        Some(serde_json::Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect(),
        Some(serde_json::Value::String(s)) => vec![s.clone()],
        _ => Vec::new(),
    }
}

pub fn module() -> Module {
    Module::new("Domains", |_ctx| {
        RuntimeGroupSpec::new(GroupSpec::new(
            "domain",
            "Domain availability and suggestions",
        ))
        .with_command(RuntimeCommandSpec::new_with_context(
            CommandSpec::new("available", "Check whether a domain is available")
                .with_system("domain")
                .with_tier(Tier::Read)
                .with_scopes(&[DOMAINS_READ_SCOPE])
                .with_arg(
                    clap::Arg::new("domain")
                        .value_name("DOMAIN")
                        .required(true)
                        .help("Domain name to check (e.g. example.com)"),
                )
                .with_arg(
                    clap::Arg::new("check-type")
                        .long("check-type")
                        .value_name("TYPE")
                        .value_parser(["fast", "full"])
                        .help("Optimize for speed (fast) or accuracy (full)"),
                )
                .with_arg(
                    clap::Arg::new("for-transfer")
                        .long("for-transfer")
                        .action(clap::ArgAction::SetTrue)
                        .help("Also include domains available for transfer"),
                ),
            |ctx| async move {
                let domain = ctx
                    .args
                    .get("domain")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_owned();
                let check_type = match ctx.args.get("check-type").and_then(|v| v.as_str()) {
                    Some(s) => Some(
                        domains_client::types::AvailableCheckType::try_from(
                            s.to_uppercase().as_str(),
                        )
                        .map_err(|_| {
                            CliCoreError::message(format!(
                                "invalid --check-type {s:?}; expected fast|full"
                            ))
                        })?,
                    ),
                    None => None,
                };
                let for_transfer = ctx
                    .args
                    .get("for-transfer")
                    .and_then(|v| v.as_bool())
                    .filter(|&b| b);

                let client = make_client(&ctx).await?;
                let resp = client
                    .available(check_type, &domain, for_transfer)
                    .await
                    .map_err(|e| {
                        CliCoreError::message(format!("domain availability check failed: {e}"))
                    })?;
                let body = resp.into_inner();

                let mut result = json!({
                    "domain": body.domain,
                    "available": body.available,
                    "definitive": body.definitive,
                });
                if let Some(price) = format_price(body.price) {
                    result["price"] = json!(price);
                    result["currency"] = json!(body.currency);
                }
                if let Some(renewal) = format_price(body.renewal_price) {
                    result["renewalPrice"] = json!(renewal);
                }
                if let Some(period) = body.period {
                    result["period"] = json!(period);
                }

                let cmd = CommandResult::new(result);
                // If it's taken, point at suggestions for the same seed.
                if body.available {
                    Ok(cmd)
                } else {
                    Ok(cmd.with_next_actions(vec![
                        NextAction::new(
                            "domain suggest <query>",
                            "Find alternative available domains",
                        )
                        .with_param("query", NextActionParam::required()),
                    ]))
                }
            },
        ))
        .with_command(RuntimeCommandSpec::new_with_context(
            CommandSpec::new("suggest", "Suggest available domains for a query")
                .with_system("domain")
                .with_tier(Tier::Read)
                .with_scopes(&[DOMAINS_READ_SCOPE])
                .with_arg(
                    clap::Arg::new("query")
                        .value_name("QUERY")
                        .required(true)
                        .help("Seed domain or keywords to base suggestions on"),
                )
                .with_arg(
                    clap::Arg::new("tlds")
                        .long("tlds")
                        .value_name("TLD")
                        .action(clap::ArgAction::Append)
                        .help("Limit suggestions to these TLDs (repeatable)"),
                )
                .with_arg(
                    clap::Arg::new("limit")
                        .long("limit")
                        .value_name("N")
                        .value_parser(clap::value_parser!(i64).range(1..))
                        .help("Maximum number of suggestions to return"),
                )
                .with_arg(
                    clap::Arg::new("country")
                        .long("country")
                        .value_name("CC")
                        .help("Two-letter ISO country hint (e.g. US)"),
                )
                .with_arg(
                    clap::Arg::new("city")
                        .long("city")
                        .value_name("CITY")
                        .help("City hint for the target region"),
                ),
            |ctx| async move {
                let query = ctx
                    .args
                    .get("query")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_owned();
                let tlds = string_list(&ctx, "tlds");
                let tlds = (!tlds.is_empty()).then_some(tlds);
                let limit = ctx.args.get("limit").and_then(|v| v.as_i64());
                let city = ctx.args.get("city").and_then(|v| v.as_str());
                let country = match ctx.args.get("country").and_then(|v| v.as_str()) {
                    Some(c) => Some(
                        domains_client::types::SuggestCountry::try_from(c.to_uppercase().as_str())
                            .map_err(|_| {
                                CliCoreError::message(format!("invalid --country {c:?}"))
                            })?,
                    ),
                    None => None,
                };

                let client = make_client(&ctx).await?;
                let resp = client
                    .suggest(
                        city,
                        country,
                        None,
                        None,
                        limit,
                        Some(&query),
                        None,
                        tlds.as_ref(),
                        None,
                        None,
                    )
                    .await
                    .map_err(|e| CliCoreError::message(format!("domain suggestion failed: {e}")))?;
                let suggestions: Vec<String> =
                    resp.into_inner().into_iter().map(|s| s.domain).collect();

                Ok(
                    CommandResult::new(json!(suggestions)).with_next_actions(vec![
                        NextAction::new("domain available <domain>", "Check a suggested domain")
                            .with_param("domain", NextActionParam::required()),
                    ]),
                )
            },
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::format_price;

    #[test]
    fn formats_micro_units_to_decimal() {
        assert_eq!(format_price(Some(11_990_000)).as_deref(), Some("11.99"));
        assert_eq!(format_price(Some(1_000_000)).as_deref(), Some("1.00"));
        assert_eq!(format_price(Some(20_500_000)).as_deref(), Some("20.50"));
        assert_eq!(format_price(None), None);
    }
}
