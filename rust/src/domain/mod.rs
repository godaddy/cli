//! `gddy domain` — domain availability and suggestions.
//!
//! These endpoints (the GoDaddy Domains API) accept either an sso-key API key or
//! an OAuth bearer token. The HTTP layer is the typed, spec-generated
//! [`domains_client`] crate; the auth scheme is chosen from the credential the
//! [`CompositeAuthProvider`](crate::auth::CompositeAuthProvider) returns for
//! `domain:*` commands — `sso-key` when a key is configured for the environment,
//! otherwise the OAuth bearer token.

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
///
/// Shared with the `dns` module: DNS record *reads* live under this same scope.
pub(crate) const DOMAINS_READ_SCOPE: &str = "domains.domain:read";

/// OAuth scope the DNS record *mutation* endpoints (PATCH/PUT/DELETE under
/// `/v1/domains/{domain}/records`) require.
///
/// Source of truth (undocumented in the published Swagger spec): the same
/// `gdcorp-domains/api-domain-data` `api/oauthscopewhitelist.json` whitelist —
/// every `PATCH`/`PUT`/`DELETE` on `v1_domains__records*` is listed under
/// `domains.dns:update` (reads stay under [`DOMAINS_READ_SCOPE`]). Consumed by
/// the `dns` add/set/delete commands.
pub(crate) const DOMAINS_DNS_UPDATE_SCOPE: &str = "domains.dns:update";

fn map_env_err(e: environments::EnvError) -> CliCoreError {
    CliCoreError::message(e.to_string())
}

/// ("11.99"). Domain prices are returned in micro-units (1 unit = 1_000_000
/// micros).
///
/// Truncates (does not round) to whole cents: the API returns whole-cent prices
/// in practice, so the sub-cent digits are always zero; truncating keeps the
/// output a faithful, surprise-free rendering of the raw value rather than
/// inventing a rounded figure. See the `formats_micro_units_to_decimal` test for
/// the defined behavior on a (synthetic) sub-cent input.
///
/// The sign is formatted explicitly (and `unsigned_abs` avoids `i64::MIN`
/// overflow) so sub-unit negatives like `-500_000` render as `-0.50`, not `0.50`.
fn format_price(micros: Option<i64>) -> Option<String> {
    micros.map(|m| {
        let sign = if m < 0 { "-" } else { "" };
        let abs = m.unsigned_abs();
        format!(
            "{sign}{}.{:02}",
            abs / 1_000_000,
            (abs % 1_000_000) / 10_000
        )
    })
}

/// Pick the `Authorization` header value for a resolved credential: the `sso-key`
/// scheme for the [`SSO_KEY_PROVIDER`] bypass path, otherwise an OAuth `Bearer`
/// token. Pure so the scheme selection is unit-testable without a full context.
fn authorization_header(provider: &str, token: &str) -> String {
    if provider == SSO_KEY_PROVIDER {
        format!("sso-key {token}")
    } else {
        format!("Bearer {token}")
    }
}

/// Build a Domains API client for the active environment, choosing the auth
/// scheme from the resolved credential (sso-key for the bypass path, else
/// Bearer). The credential is resolved through the registered composite provider.
pub(crate) async fn make_client(ctx: &CommandContext) -> Result<domains_client::Client> {
    let env = ctx.middleware.env.clone();
    let domains = environments::resolve_domains(&env).map_err(map_env_err)?;
    let cred = ctx.credential().await?;
    let authorization = authorization_header(&cred.provider, &cred.token);
    let request_id = uuid::Uuid::new_v4().to_string();
    domains_client::client_with_auth(&domains.base_url, &authorization, USER_AGENT, &request_id)
        .map_err(|e| CliCoreError::message(format!("failed to build domains client: {e}")))
}

pub(crate) fn string_list(ctx: &CommandContext, key: &str) -> Vec<String> {
    match ctx.args.get(key) {
        Some(serde_json::Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect(),
        Some(serde_json::Value::String(s)) => vec![s.clone()],
        _ => Vec::new(),
    }
}

/// Validate `--status` values case-insensitively against the generated
/// `ListStatusesItem` enum (the API's `DomainStatus` set, e.g. `ACTIVE`),
/// returning the typed list the `list` builder expects.
fn parse_statuses(raw: &[String]) -> Result<Vec<domains_client::types::ListStatusesItem>> {
    raw.iter()
        .map(|s| {
            domains_client::types::ListStatusesItem::try_from(s.to_uppercase().as_str())
                .map_err(|_| CliCoreError::message(format!("invalid --status {s:?}")))
        })
        .collect()
}

pub fn module() -> Module {
    Module::new("Domains", |_ctx| {
        RuntimeGroupSpec::new(GroupSpec::new(
            "domain",
            "List your domains, check availability, and get suggestions",
        ))
        .with_command(RuntimeCommandSpec::new_with_context(
            CommandSpec::new("list", "List the domains in your account")
                .with_system("domain")
                .with_tier(Tier::Read)
                .with_default_fields("domain,status,expires,renewAuto")
                .with_json_schema::<domains_client::types::DomainSummary>()
                .with_scopes(&[DOMAINS_READ_SCOPE])
                .with_arg(
                    clap::Arg::new("status")
                        .long("status")
                        .value_name("STATUS")
                        .action(clap::ArgAction::Append)
                        .help("Only domains with this status, e.g. ACTIVE (repeatable)"),
                ),
            |ctx| async move {
                let statuses = parse_statuses(&string_list(&ctx, "status"))?;

                let client = make_client(&ctx).await?;
                let mut req = client.list();
                if !statuses.is_empty() {
                    req = req.statuses(statuses);
                }
                let resp = req
                    .send()
                    .await
                    .map_err(|e| CliCoreError::message(format!("listing domains failed: {e}")))?;

                // Emit each summary as an object so `--fields`/default-field
                // projection works (default shows domain/status/expires/renewAuto).
                let domains: Vec<serde_json::Value> = resp
                    .into_inner()
                    .iter()
                    .map(|d| serde_json::to_value(d).unwrap_or_else(|_| json!({})))
                    .collect();

                Ok(CommandResult::new(json!(domains)).with_next_actions(vec![
                    NextAction::new("dns list <domain>", "View a domain's DNS records")
                        .with_param("domain", NextActionParam::required()),
                ]))
            },
        ))
        .with_command(RuntimeCommandSpec::new_with_context(
            CommandSpec::new("available", "Check whether a domain is available")
                .with_system("domain")
                .with_tier(Tier::Read)
                .with_default_fields("domain,available,definitive,price,currency")
                .with_json_schema::<domains_client::types::DomainAvailableResponse>()
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
                let mut req = client.available().domain(domain.as_str());
                if let Some(ct) = check_type {
                    req = req.check_type(ct);
                }
                if let Some(ft) = for_transfer {
                    req = req.for_transfer(ft);
                }
                let resp = req.send().await.map_err(|e| {
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
                .with_default_fields("domain")
                .with_json_schema::<domains_client::types::DomainSuggestion>()
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
                let mut req = client.suggest().query(query.as_str());
                if let Some(n) = limit {
                    req = req.limit(n);
                }
                if let Some(t) = tlds {
                    req = req.tlds(t);
                }
                if let Some(c) = country {
                    req = req.country(c);
                }
                if let Some(city) = city {
                    req = req.city(city);
                }
                let resp = req
                    .send()
                    .await
                    .map_err(|e| CliCoreError::message(format!("domain suggestion failed: {e}")))?;
                // Emit objects (not bare strings) so the list has a projectable
                // `domain` field for `--fields`/default-field rendering.
                let suggestions: Vec<serde_json::Value> = resp
                    .into_inner()
                    .into_iter()
                    .map(|s| json!({ "domain": s.domain }))
                    .collect();

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
    use super::{authorization_header, format_price, parse_statuses};
    use crate::auth::SSO_KEY_PROVIDER;
    use cli_engine::{Cli, CliConfig};

    #[test]
    fn formats_micro_units_to_decimal() {
        assert_eq!(format_price(Some(11_990_000)).as_deref(), Some("11.99"));
        assert_eq!(format_price(Some(1_000_000)).as_deref(), Some("1.00"));
        assert_eq!(format_price(Some(20_500_000)).as_deref(), Some("20.50"));
        // Negatives keep their sign, including sub-unit amounts.
        assert_eq!(format_price(Some(-11_990_000)).as_deref(), Some("-11.99"));
        assert_eq!(format_price(Some(-500_000)).as_deref(), Some("-0.50"));
        assert_eq!(format_price(None), None);
        // Sub-cent micros truncate toward the lower cent (documented behavior):
        // 1_005_000 micros = 1.005 -> "1.00", never "1.01".
        assert_eq!(format_price(Some(1_005_000)).as_deref(), Some("1.00"));
    }

    #[test]
    fn authorization_header_picks_scheme_from_provider() {
        // sso-key bypass path -> `sso-key KEY:SECRET`.
        assert_eq!(
            authorization_header(SSO_KEY_PROVIDER, "KEY:SECRET"),
            "sso-key KEY:SECRET"
        );
        // Any other provider (OAuth/PKCE) -> `Bearer <token>`.
        assert_eq!(authorization_header("godaddy", "tok123"), "Bearer tok123");
    }

    #[test]
    fn parse_statuses_is_case_insensitive_and_validates() {
        use domains_client::types::ListStatusesItem;
        let parsed = parse_statuses(&["active".to_string(), "CANCELLED".to_string()])
            .expect("valid statuses");
        assert_eq!(
            parsed,
            vec![ListStatusesItem::Active, ListStatusesItem::Cancelled]
        );
        // Empty input is valid (no filter).
        assert!(parse_statuses(&[]).expect("empty ok").is_empty());
        // Unknown status is rejected with a helpful message.
        let err = parse_statuses(&["bogus".to_string()]).expect_err("should reject");
        assert!(err.to_string().contains("invalid --status"), "{err}");
    }

    /// The `domain` commands call the Domains API, so they must stay fail-closed.
    /// Built with **no auth provider registered**, the engine's default
    /// `AuthRequirement::Required` must reject them at credential resolution
    /// (exit code 2, provider error) before the handler runs — guarding against
    /// anyone marking them `no_auth(true)` and letting them run unauthenticated.
    #[tokio::test]
    async fn domain_commands_require_auth() {
        const AUTH_FAILURE_EXIT: i32 = 2;
        // No `--env` flag here: the global flag is registered in main.rs, not in
        // this minimal test harness, and env is irrelevant since auth resolution
        // fails before the handler runs.
        let cases: [&[&str]; 3] = [
            &["gddy", "domain", "list", "--output", "json"],
            &[
                "gddy",
                "domain",
                "available",
                "example.com",
                "--output",
                "json",
            ],
            &["gddy", "domain", "suggest", "coffee", "--output", "json"],
        ];
        for args in cases {
            let cli = Cli::new(
                CliConfig::new("gddy", "GoDaddy developer CLI", "gddy")
                    .with_default_auth_provider("godaddy")
                    .with_module(super::module()),
            );
            let output = cli.run(args.iter().copied()).await;
            assert_eq!(
                output.exit_code, AUTH_FAILURE_EXIT,
                "{args:?} must fail closed at auth resolution, got: {}",
                output.rendered
            );
            let json: serde_json::Value =
                serde_json::from_str(&output.rendered).expect("valid json output");
            let message = json["error"]["message"].as_str().unwrap_or_default();
            assert!(
                message.contains("provider"),
                "expected an auth-provider resolution error for {args:?}, got: {}",
                output.rendered
            );
        }
    }
}
