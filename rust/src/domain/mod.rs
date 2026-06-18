//! `gddy domain` — domain availability and suggestions.
//!
//! These endpoints (the GoDaddy Domains API) accept either an sso-key API key or
//! an OAuth bearer token. The HTTP layer is the typed, spec-generated
//! [`domains_client`] crate; the auth scheme is chosen from the credential the
//! [`CompositeAuthProvider`](crate::auth::CompositeAuthProvider) returns for
//! `domain:*` commands — `sso-key` when a key is configured for the environment,
//! otherwise the OAuth bearer token.

use cli_engine::{
    CliCoreError, CommandContext, CommandResult, CommandSpec, Credential, GroupSpec, Module,
    NextAction, NextActionParam, Result, RuntimeCommandSpec, RuntimeGroupSpec, Tier,
};
use serde_json::json;

use crate::{
    auth::SSO_KEY_PROVIDER,
    contacts::{self, Role},
    environments,
    output_schema::output_schema,
};

output_schema!(DomainPurchaseResult {
    "domain": "string";
    "status": "string";
    "price": "string";
    "currency": "string";
});

// `domain get` returns the full `DomainDetail` as free-form JSON; this documents
// the commonly-projected fields for `--schema` and the default-field table.
output_schema!(DomainDetailResult {
    "domain": "string";
    "status": "string";
    "expires": "string";
    "renewAuto": "bool";
    "nameServers": "[]string";
    "privacy": "bool";
    "locked": "bool";
    "createdAt": "string";
});

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

/// OAuth scope the domain *purchase* endpoint requires. `domain purchase` uses
/// the v2 register API (`POST /v2/customers/{customerId}/domains/register`),
/// which — unlike v1 `/v1/domains/purchase` — authorizes card payments for OAuth
/// users.
///
/// Source of truth (undocumented in the published Swagger spec): the same
/// `gdcorp-domains/api-domain-data` `api/oauthscopewhitelist.json` whitelist
/// lists both `POST.v2_customers__domains_register` and `POST.v1_domains_purchase`
/// under `domains.domain:create` (the legal agreements GET that precedes it stays
/// under [`DOMAINS_READ_SCOPE`]). Consumed by the `domain purchase` command.
pub(crate) const DOMAINS_PURCHASE_SCOPE: &str = "domains.domain:create";

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

/// The registrable TLD of a domain: everything after the first label
/// (`example.com` → `com`, `example.co.uk` → `co.uk`). `None` when there is no
/// dot, or when either the first label or the TLD is empty (`.com`, `example.`)
/// — those aren't registrable domains and would otherwise produce invalid
/// downstream lookups (e.g. an empty-TLD schema fetch).
fn registrable_tld(domain: &str) -> Option<&str> {
    let (label, tld) = domain.split_once('.')?;
    (!label.is_empty() && !tld.is_empty()).then_some(tld)
}

/// Format a UTC instant as the Domains API's `iso-datetime` for purchase consent
/// (`consent.agreedAt`): RFC 3339 with a literal trailing `Z`, e.g.
/// `2026-06-17T22:34:43Z`.
///
/// The API enforces `…THH:MM:SS(.fraction)?Z$` and rejects the numeric-offset
/// form (`+00:00`) that `chrono`'s `to_rfc3339()` emits — so we pin the `Z`
/// (`use_z = true`) and drop sub-second digits.
fn iso_datetime(now: chrono::DateTime<chrono::Utc>) -> String {
    now.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// The GoDaddy customer id (the `{customerId}` path segment the v2 register API
/// requires), taken from the OAuth token's `sub` claim (which cli-engine parses
/// onto the credential).
///
/// `sub` is a typed subject URN — `customer:<uuid>` for the customer tokens this
/// command needs — and the v2 path wants the bare uuid, so the `customer:`
/// prefix is stripped. A subject that isn't `customer:`-typed (or an empty one)
/// isn't a customer identity, so it's rejected with a clear error.
fn customer_id(cred: &Credential) -> Result<String> {
    cred.sub
        .strip_prefix("customer:")
        .filter(|uuid| !uuid.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| {
            CliCoreError::message(format!(
                "the OAuth token's subject ({:?}) is not a customer identity; `domain purchase` \
                 needs a customer-scoped token",
                cred.sub
            ))
        })
}

/// Enforce the purchase gates and return the agreement keys to record as consent.
///
/// Pure (no I/O) so the gating is unit-testable: it takes the already-fetched
/// legal agreements and the `--agree`/`--confirm` flags. `--agree` is the legal
/// consent gate (its error lists the agreements to review); `--confirm` is the
/// charge gate (the purchase is paid and irreversible). The agreement keys are
/// only returned once both gates pass.
fn purchase_consent_keys(
    domain: &str,
    tld: &str,
    period: u64,
    agree: bool,
    confirm: bool,
    agreements: &[domains_client::types::LegalAgreement],
) -> Result<Vec<String>> {
    if !agree {
        let list = agreements
            .iter()
            .map(|a| {
                let title = a.title.as_deref().unwrap_or("(untitled agreement)");
                match a.url.as_deref() {
                    Some(url) => format!("  - {title}: {url}"),
                    None => format!("  - {title}"),
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        return Err(CliCoreError::message(format!(
            "registering .{tld} requires agreeing to its legal agreement(s):\n{list}\n\n\
             Review the full text with `gddy domain agreements --tld {tld}`, then re-run with \
             --agree. See `gddy guide domain-purchase`."
        )));
    }

    let keys: Vec<String> = agreements
        .iter()
        .filter_map(|a| a.agreement_key.clone())
        .collect();
    if keys.is_empty() {
        return Err(CliCoreError::message(format!(
            "no legal agreement keys were returned for .{tld}; cannot record consent for {domain}"
        )));
    }

    if !confirm {
        return Err(CliCoreError::message(format!(
            "purchasing {domain} for {period} year(s) charges your account and cannot be undone; \
             re-run with --confirm to proceed (or --dry-run to preview)"
        )));
    }

    Ok(keys)
}

/// Turn a domains-client error into a `CliCoreError`, reading the response body
/// for unexpected (non-2xx) responses. progenitor's `Error::UnexpectedResponse`
/// `Display` prints only the response status/headers — never the body — so the
/// API's actual message (e.g. a 422's per-field reason) is otherwise lost. This
/// recovers it. Async because reading the body is async, so call sites match on
/// the result rather than using `.map_err`.
async fn api_error(action: &str, debug: bool, err: domains_client::Error<()>) -> CliCoreError {
    match err {
        domains_client::Error::UnexpectedResponse(resp) => {
            let status = resp.status();
            // The server echoes the `x-request-id` we send; read it before the
            // body is consumed. Only surfaced under `--debug` (it's the handle
            // for correlating a failed request server-side).
            let request_id = resp
                .headers()
                .get("x-request-id")
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned);
            let body = resp.text().await.unwrap_or_default();
            CliCoreError::message(format_api_error(
                action,
                status.as_u16(),
                &status.to_string(),
                &body,
                request_id.as_deref(),
                debug,
            ))
        }
        other => CliCoreError::message(format!("{action} failed: {other}")),
    }
}

/// Build the user-facing message for an unexpected API response. Pure so the
/// HTTP 402 → payment-method guidance and the `--debug` request-id line are
/// unit-testable. `status_display` is the full status line (e.g.
/// `"402 Payment Required"`); `status` is its numeric code for matching;
/// `request_id` is the response's `x-request-id`, surfaced only when `debug`.
///
/// A 402 on purchase means payment authorization failed (no usable payment
/// method, or it was declined), so we always point users at `gddy payments add`.
/// The request id stays behind `--debug` to keep ordinary errors clean.
fn format_api_error(
    action: &str,
    status: u16,
    status_display: &str,
    body: &str,
    request_id: Option<&str>,
    debug: bool,
) -> String {
    let body = body.trim();
    let mut msg = if body.is_empty() {
        format!("{action} failed (HTTP {status_display})")
    } else {
        format!("{action} failed (HTTP {status_display}): {body}")
    };
    if status == 402 {
        msg.push_str(
            "\n\nThis usually means your account has no usable payment method. Add one with \
             `gddy payments add` (a credit card or Good-as-Gold balance is required for domain \
             purchases), then try again.",
        );
    }
    if debug && let Some(id) = request_id.map(str::trim).filter(|s| !s.is_empty()) {
        msg.push_str(&format!("\n\nRequest ID: {id}"));
    }
    msg
}

/// `DomainPurchase` request fields this CLI can populate (other than the four
/// contacts, which are checked separately), named as the per-TLD schema names
/// them. A `required` field outside this set + the contacts is something we
/// can't supply yet.
const SENDABLE_PURCHASE_FIELDS: &[&str] = &[
    "domain",
    "consent",
    "period",
    "privacy",
    "renewAuto",
    "nameServers",
];

/// Preflight a purchase against the TLD's required fields (the top-level
/// `required` array from `GET /v1/domains/purchase/schema/{tld}`).
///
/// Pure (no I/O) so it's unit-testable. `present_contacts` are the roles we will
/// actually send (resolved from contacts.toml); the always-sent fields
/// (domain/consent/period/privacy/renewAuto) never block. Blocks *before* the
/// paid call when a required contact isn't being sent, or when a required field
/// falls outside what this CLI can supply — per the "block with a clear error"
/// decision, so a doomed purchase is never attempted.
fn check_tld_requirements(tld: &str, required: &[String], present_contacts: &[Role]) -> Result<()> {
    for field in required {
        let contact_role = match field.as_str() {
            "contactRegistrant" => Some(Role::Registrant),
            "contactAdmin" => Some(Role::Admin),
            "contactBilling" => Some(Role::Billing),
            "contactTech" => Some(Role::Tech),
            _ => None,
        };
        if let Some(role) = contact_role {
            if !present_contacts.contains(&role) {
                let label = role.label();
                return Err(CliCoreError::message(format!(
                    "registering .{tld} requires a {label} contact, but none is configured; \
                     add a [{label}] section to contacts.toml (run `gddy domain contacts init` to \
                     scaffold one). See `gddy guide domain-purchase`."
                )));
            }
            continue;
        }
        if SENDABLE_PURCHASE_FIELDS.contains(&field.as_str()) {
            continue;
        }
        return Err(CliCoreError::message(format!(
            "registering .{tld} requires '{field}', which this CLI can't supply yet; \
             inspect the full requirements with `gddy domain schema {tld}`"
        )));
    }
    Ok(())
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
                    .map(serde_json::to_value)
                    .collect::<std::result::Result<_, _>>()
                    .map_err(|e| {
                        CliCoreError::message(format!("failed to serialize domain list: {e}"))
                    })?;

                Ok(CommandResult::new(json!(domains)).with_next_actions(vec![
                    NextAction::new("dns list <domain>", "View a domain's DNS records")
                        .with_param("domain", NextActionParam::required()),
                ]))
            },
        ))
        .with_command(RuntimeCommandSpec::new_with_context(
            CommandSpec::new("get", "Show full details for one of your domains")
                .with_system("domain")
                .with_tier(Tier::Read)
                // No default fields: `get` is a single-domain deep-dive, so show
                // every detail by default (an empty selection keeps all fields).
                // `list` keeps its abbreviated default; `get` does not.
                .with_output_schema::<DomainDetailResult>()
                .with_scopes(&[DOMAINS_READ_SCOPE])
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
                let detail = match client.get().domain(domain.as_str()).send().await {
                    Ok(r) => r.into_inner(),
                    Err(e) => return Err(api_error("retrieving domain details", debug, e).await),
                };
                Ok(
                    CommandResult::new(serde_json::Value::Object(detail)).with_next_actions(vec![
                        NextAction::new("dns list <domain>", "View this domain's DNS records")
                            .with_param("domain", NextActionParam::required()),
                    ]),
                )
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
        .with_command(RuntimeCommandSpec::new_with_context(
            CommandSpec::new(
                "agreements",
                "Show the legal agreements required to register a TLD",
            )
            .with_system("domain")
            .with_tier(Tier::Read)
            .with_default_fields("agreementKey,title,url")
            .with_json_schema::<domains_client::types::LegalAgreement>()
            .with_scopes(&[DOMAINS_READ_SCOPE])
            .with_arg(
                clap::Arg::new("tld")
                    .long("tld")
                    .value_name("TLD")
                    .required(true)
                    .action(clap::ArgAction::Append)
                    .help("TLD whose agreements to retrieve, e.g. com (repeatable)"),
            )
            .with_arg(
                clap::Arg::new("privacy")
                    .long("privacy")
                    .action(clap::ArgAction::SetTrue)
                    .help("Retrieve the agreements that apply when privacy is requested"),
            )
            .with_arg(
                clap::Arg::new("for-transfer")
                    .long("for-transfer")
                    .action(clap::ArgAction::SetTrue)
                    .help("Retrieve the agreements that apply to a transfer"),
            ),
            |ctx| async move {
                let tlds = string_list(&ctx, "tld");
                let privacy = ctx
                    .args
                    .get("privacy")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let for_transfer = ctx
                    .args
                    .get("for-transfer")
                    .and_then(|v| v.as_bool())
                    .filter(|&b| b);

                let debug = !ctx.middleware.debug.is_empty();
                let client = make_client(&ctx).await?;
                let mut req = client.agreements().tlds(tlds).privacy(privacy);
                if let Some(ft) = for_transfer {
                    req = req.for_transfer(ft);
                }
                let resp = match req.send().await {
                    Ok(r) => r,
                    Err(e) => return Err(api_error("retrieving legal agreements", debug, e).await),
                };

                // Emit objects with a projectable shape; `content` (the full
                // agreement text) is carried through for `--fields content` but
                // hidden by the default fields.
                let agreements: Vec<serde_json::Value> = resp
                    .into_inner()
                    .into_iter()
                    .map(|a| {
                        json!({
                            "agreementKey": a.agreement_key,
                            "title": a.title,
                            "url": a.url,
                            "content": a.content,
                        })
                    })
                    .collect();

                Ok(CommandResult::new(json!(agreements)))
            },
        ))
        .with_command(RuntimeCommandSpec::new_with_context(
            CommandSpec::new(
                "schema",
                "Show a TLD's requirements for registering a domain",
            )
            .with_system("domain")
            .with_tier(Tier::Read)
            .with_default_fields("required")
            .with_scopes(&[DOMAINS_READ_SCOPE])
            .with_arg(
                clap::Arg::new("tld")
                    .value_name("TLD")
                    .required(true)
                    .help("TLD to inspect, e.g. fun"),
            ),
            |ctx| async move {
                let tld = ctx
                    .args
                    .get("tld")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_owned();
                let debug = !ctx.middleware.debug.is_empty();
                let client = make_client(&ctx).await?;
                let schema = match client.schema().tld(tld.as_str()).send().await {
                    Ok(r) => r.into_inner(),
                    Err(e) => {
                        return Err(api_error("retrieving the TLD purchase schema", debug, e).await);
                    }
                };
                Ok(CommandResult::new(serde_json::Value::Object(schema)))
            },
        ))
        .with_command(RuntimeCommandSpec::new_with_context(
            CommandSpec::new("purchase", "Register a domain (paid; charges your account)")
                .with_system("domain")
                .with_tier(Tier::Destructive)
                .with_default_fields("domain,status,price,currency")
                .with_output_schema::<DomainPurchaseResult>()
                // The flow reads the TLD schema, availability, and agreements
                // (all `domains.domain:read`) before registering (`:create`), so
                // the minted token must carry both scopes.
                .with_scopes(&[DOMAINS_READ_SCOPE, DOMAINS_PURCHASE_SCOPE])
                .with_arg(
                    clap::Arg::new("domain")
                        .value_name("DOMAIN")
                        .required(true)
                        .help("Domain to register, e.g. example.com"),
                )
                .with_arg(
                    clap::Arg::new("period")
                        .long("period")
                        .value_name("YEARS")
                        .value_parser(clap::value_parser!(u64).range(1..=10))
                        .default_value("1")
                        .help("Registration length in years (1-10)"),
                )
                .with_arg(
                    clap::Arg::new("privacy")
                        .long("privacy")
                        .action(clap::ArgAction::SetTrue)
                        .help("Add privacy protection to the registration"),
                )
                .with_arg(
                    clap::Arg::new("no-renew")
                        .long("no-renew")
                        .action(clap::ArgAction::SetTrue)
                        .help("Disable auto-renewal (auto-renew is on by default)"),
                )
                .with_arg(
                    clap::Arg::new("nameserver")
                        .long("nameserver")
                        .value_name("HOST")
                        .action(clap::ArgAction::Append)
                        .help("Custom nameserver (repeatable); omit to use GoDaddy defaults"),
                )
                .with_arg(
                    clap::Arg::new("agree")
                        .long("agree")
                        .action(clap::ArgAction::SetTrue)
                        .help(
                            "Consent to the TLD's legal agreements (review with \
                         `domain agreements` or `gddy guide domain-purchase`)",
                        ),
                )
                .with_arg(
                    clap::Arg::new("agreed-by")
                        .long("agreed-by")
                        .value_name("IP")
                        .help("Originating IP recorded with your consent (defaults to 127.0.0.1)"),
                )
                .with_arg(
                    clap::Arg::new("confirm")
                        .long("confirm")
                        .action(clap::ArgAction::SetTrue)
                        .help("Confirm the purchase; required because it charges your account"),
                ),
            |ctx| async move {
                let domain = ctx
                    .args
                    .get("domain")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_owned();
                let Some(tld) = registrable_tld(&domain).map(str::to_owned) else {
                    return Err(CliCoreError::message(format!(
                        "{domain:?} is not a registrable domain (expected e.g. example.com)"
                    )));
                };
                let period = ctx.args.get("period").and_then(|v| v.as_u64()).unwrap_or(1);
                let privacy = ctx
                    .args
                    .get("privacy")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let renew_auto = !ctx
                    .args
                    .get("no-renew")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let name_servers = string_list(&ctx, "nameserver");
                let agree = ctx
                    .args
                    .get("agree")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let confirm = ctx
                    .args
                    .get("confirm")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let agreed_by = ctx
                    .args
                    .get("agreed-by")
                    .and_then(|v| v.as_str())
                    .unwrap_or("127.0.0.1")
                    .to_owned();

                let debug = !ctx.middleware.debug.is_empty();

                // Validate auth before anything else: purchase goes through the v2
                // register API, which (unlike v1) authorizes card payments for OAuth
                // users and needs the customerId from the OAuth token. sso-key auth
                // can never succeed here, so fail with that clear message *before*
                // touching contacts.toml (whose parse errors would otherwise mask it).
                let cred = ctx.credential().await?;
                if cred.provider == SSO_KEY_PROVIDER {
                    return Err(CliCoreError::message(
                        "`domain purchase` uses the v2 registration API, which requires OAuth \
                         authentication; this environment is configured for an sso-key. Re-run \
                         against an OAuth environment.",
                    ));
                }
                let customer_id = customer_id(&cred)?;

                // Resolve default contacts from local config (still before any
                // network call). Absent roles stay None and the API uses the
                // account-default contact.
                let contacts =
                    contacts::load().map_err(|e| CliCoreError::message(e.to_string()))?;
                let contact_registrant = contacts
                    .to_api(Role::Registrant)
                    .map_err(CliCoreError::message)?;
                let contact_admin = contacts
                    .to_api(Role::Admin)
                    .map_err(CliCoreError::message)?;
                let contact_billing = contacts
                    .to_api(Role::Billing)
                    .map_err(CliCoreError::message)?;
                let contact_tech = contacts.to_api(Role::Tech).map_err(CliCoreError::message)?;

                let client = make_client(&ctx).await?;

                // Preflight: the TLD's purchase schema lists which fields it
                // requires. Block before the paid call when a required contact
                // isn't being sent (or a field we can't supply is required),
                // rather than letting the register POST come back 422.
                let present_contacts: Vec<Role> = [
                    (Role::Registrant, &contact_registrant),
                    (Role::Admin, &contact_admin),
                    (Role::Billing, &contact_billing),
                    (Role::Tech, &contact_tech),
                ]
                .into_iter()
                .filter_map(|(role, c)| c.is_some().then_some(role))
                .collect();
                let schema = match client.schema().tld(tld.as_str()).send().await {
                    Ok(r) => r.into_inner(),
                    Err(e) => {
                        return Err(api_error("retrieving the TLD purchase schema", debug, e).await);
                    }
                };
                let required: Vec<String> = schema
                    .get("required")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(str::to_owned))
                            .collect()
                    })
                    .unwrap_or_default();
                check_tld_requirements(&tld, &required, &present_contacts)?;

                // Price + currency: the v2 consent records the price the user
                // acknowledged (per the API, sourced from GET /v1/domains/available).
                let availability = match client.available().domain(domain.as_str()).send().await {
                    Ok(r) => r.into_inner(),
                    Err(e) => {
                        return Err(api_error("checking domain availability", debug, e).await);
                    }
                };
                // Fail fast on a taken domain: availability can carry a price even
                // when `available` is false, so without this the command would
                // proceed to a guaranteed-failing paid register.
                if !availability.available {
                    return Err(CliCoreError::message(format!(
                        "{domain} is not available for registration"
                    )));
                }
                let price = availability.price.ok_or_else(|| {
                    CliCoreError::message(format!(
                        "could not determine a price for {domain}; it may be premium or \
                         not offered for registration"
                    ))
                })?;
                let currency = availability.currency;

                // The legal agreements for the TLD: their keys are the consent
                // record, and (when --agree is missing) the list shown to review.
                let agreements = match client
                    .agreements()
                    .tlds(vec![tld.clone()])
                    .privacy(privacy)
                    .send()
                    .await
                {
                    Ok(r) => r.into_inner(),
                    Err(e) => return Err(api_error("retrieving legal agreements", debug, e).await),
                };

                let agreement_keys =
                    purchase_consent_keys(&domain, &tld, period, agree, confirm, &agreements)?;

                let consent = domains_client::types::ConsentV2 {
                    agreed_at: iso_datetime(chrono::Utc::now()),
                    agreed_by,
                    agreement_keys,
                    claim_token: None,
                    currency: currency.clone(),
                    price,
                    registry_premium_pricing: None,
                };
                // Only send a contacts object when at least one role is configured;
                // otherwise omit it so the API uses the account-default contacts.
                let contacts_body = domains_client::types::DomainContactsCreateV2 {
                    registrant: contact_registrant,
                    registrant_id: None,
                    admin: contact_admin,
                    admin_id: None,
                    billing: contact_billing,
                    billing_id: None,
                    tech: contact_tech,
                    tech_id: None,
                };
                let has_contacts = contacts_body.registrant.is_some()
                    || contacts_body.admin.is_some()
                    || contacts_body.billing.is_some()
                    || contacts_body.tech.is_some();
                let period = std::num::NonZeroU64::new(period)
                    .expect("clap value_parser enforces period >= 1");
                let purchase = domains_client::types::DomainPurchaseV2 {
                    consent,
                    contacts: has_contacts.then_some(contacts_body),
                    domain: domain.clone(),
                    metadata: Default::default(),
                    name_servers,
                    period,
                    privacy,
                    renew_auto,
                };

                // 202 Accepted with no body — registration proceeds asynchronously.
                if let Err(e) = client
                    .register()
                    .customer_id(customer_id.as_str())
                    .body(purchase)
                    .send()
                    .await
                {
                    let err = api_error("domain purchase", debug, e).await;
                    if debug {
                        // Surface the customer-scoped path we POSTed to — the most
                        // common cause of a register failure is the wrong
                        // customerId.
                        return Err(CliCoreError::message(format!(
                            "{err}\n[debug] request: POST /v2/customers/{customer_id}/domains/register"
                        )));
                    }
                    return Err(err);
                }

                let result = json!({
                    "domain": domain,
                    "status": "submitted",
                    "price": format_price(Some(price)),
                    "currency": currency,
                });
                Ok(
                    CommandResult::new(result).with_next_actions(vec![NextAction::new(
                        "domain list",
                        "See your registered domains (registration completes asynchronously)",
                    )]),
                )
            },
        ))
        .with_group(
            RuntimeGroupSpec::new(GroupSpec::new(
                "contacts",
                "Manage saved default contacts for domain purchases",
            ))
            .with_command(RuntimeCommandSpec::new_with_context(
                CommandSpec::new("init", "Write a starter contacts.toml you can edit")
                    .with_system("domain")
                    // Local file write, no API call: dry-run aware, no auth.
                    .with_tier(Tier::Mutate)
                    .mutates(true)
                    .no_auth(true)
                    .with_default_fields("path,action")
                    .with_arg(
                        clap::Arg::new("force")
                            .long("force")
                            .action(clap::ArgAction::SetTrue)
                            .help("Overwrite an existing contacts.toml"),
                    ),
                |ctx| async move {
                    let path = contacts::contacts_path().ok_or_else(|| {
                        CliCoreError::message(
                            "could not determine a config directory for contacts.toml",
                        )
                    })?;
                    let force = ctx
                        .args
                        .get("force")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    let existed = path.exists();
                    if existed && !force {
                        return Err(CliCoreError::message(format!(
                            "{} already exists; pass --force to overwrite",
                            path.display()
                        )));
                    }
                    cli_engine::fs::write_string_atomic(&path, contacts::sample_toml())?;

                    Ok(CommandResult::new(json!({
                        "path": path.display().to_string(),
                        // Base on prior existence, not the flag: `--force` on a
                        // missing file still creates rather than overwrites.
                        "action": if existed { "overwritten" } else { "created" },
                    }))
                    .with_next_actions(vec![NextAction::new(
                        "guide domain-purchase",
                        "Learn how purchase uses these contacts",
                    )]))
                },
            )),
        )
    })
    // Long-form purchase walkthrough (consent gates + saved default contacts),
    // surfaced as `gddy guide domain-purchase`. The command help and the
    // missing-`--agree` error both point here.
    .with_guides_from_markdown([(
        "domain-purchase.md",
        include_bytes!("guides/domain-purchase.md").as_slice(),
    )])
}

#[cfg(test)]
mod tests {
    use super::{
        authorization_header, check_tld_requirements, customer_id, format_api_error, format_price,
        iso_datetime, parse_statuses, purchase_consent_keys, registrable_tld,
    };
    use crate::auth::SSO_KEY_PROVIDER;
    use crate::contacts::Role;
    use cli_engine::Credential;
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

    #[test]
    fn registrable_tld_takes_everything_after_first_label() {
        assert_eq!(registrable_tld("example.com"), Some("com"));
        assert_eq!(registrable_tld("example.co.uk"), Some("co.uk"));
        // No dot, or an empty label/TLD -> not a registrable domain.
        assert_eq!(registrable_tld("localhost"), None);
        assert_eq!(registrable_tld(".com"), None);
        assert_eq!(registrable_tld("example."), None);
    }

    fn agreement(
        key: &str,
        title: &str,
        url: Option<&str>,
    ) -> domains_client::types::LegalAgreement {
        domains_client::types::LegalAgreement {
            agreement_key: Some(key.to_string()),
            title: Some(title.to_string()),
            url: url.map(str::to_string),
            content: None,
        }
    }

    #[test]
    fn purchase_consent_requires_agree_then_confirm() {
        let agreements = vec![agreement(
            "DNRA",
            "Domain Registration Agreement",
            Some("https://example.com/dnra"),
        )];

        // Without --agree: error lists the agreement(s) and points to review.
        let err = purchase_consent_keys("example.com", "com", 1, false, true, &agreements)
            .expect_err("must require --agree");
        let msg = err.to_string();
        assert!(msg.contains("Domain Registration Agreement"), "{msg}");
        assert!(msg.contains("--agree"), "{msg}");

        // With --agree but not --confirm: error is about the charge.
        let err = purchase_consent_keys("example.com", "com", 2, true, false, &agreements)
            .expect_err("must require --confirm");
        let msg = err.to_string();
        assert!(msg.contains("--confirm"), "{msg}");
        assert!(msg.contains("2 year(s)"), "{msg}");

        // Both gates passed: returns the agreement keys to record as consent.
        let keys = purchase_consent_keys("example.com", "com", 1, true, true, &agreements)
            .expect("both gates satisfied");
        assert_eq!(keys, vec!["DNRA".to_string()]);
    }

    #[test]
    fn purchase_consent_rejects_when_no_agreement_keys() {
        // --agree given, but the API returned no usable keys: consent can't be
        // recorded, so the purchase must not proceed even with --confirm.
        let err = purchase_consent_keys("example.com", "com", 1, true, true, &[])
            .expect_err("no keys -> error");
        assert!(err.to_string().contains("no legal agreement keys"), "{err}");
    }

    #[test]
    fn payment_required_error_points_to_payments_add() {
        // Payment guidance is user UX, not debug — it shows without --debug.
        let msg = format_api_error(
            "domain purchase",
            402,
            "402 Payment Required",
            r#"{"code":"INVALID_PAYMENT_INFO","message":"Unable to authorize credit"}"#,
            None,
            false,
        );
        assert!(msg.contains("402 Payment Required"), "{msg}");
        assert!(msg.contains("INVALID_PAYMENT_INFO"), "{msg}"); // original body preserved
        assert!(msg.contains("gddy payments add"), "{msg}");
    }

    #[test]
    fn non_payment_errors_have_no_payment_hint() {
        let msg = format_api_error(
            "domain purchase",
            422,
            "422 Unprocessable Entity",
            "body.consent.agreedAt bad format",
            None,
            false,
        );
        assert!(msg.contains("agreedAt"), "{msg}");
        assert!(!msg.contains("payments add"), "{msg}");
        // Empty body omits the trailing colon.
        let empty = format_api_error("domain schema", 404, "404 Not Found", "", None, false);
        assert!(empty.ends_with("(HTTP 404 Not Found)"), "{empty}");
    }

    #[test]
    fn request_id_is_gated_behind_debug() {
        let id = Some("93f95de0-9379-4313-9798-bb8a49874724");

        // Without --debug: clean output, no request id even when present.
        let plain = format_api_error(
            "domain purchase",
            402,
            "402 Payment Required",
            "{}",
            id,
            false,
        );
        assert!(!plain.contains("Request ID"), "{plain}");
        assert!(plain.contains("gddy payments add"), "{plain}"); // payment hint still shows

        // With --debug: request id is appended (plainly, no editorializing).
        let debug = format_api_error(
            "domain purchase",
            402,
            "402 Payment Required",
            "{}",
            id,
            true,
        );
        assert!(
            debug.contains("Request ID: 93f95de0-9379-4313-9798-bb8a49874724"),
            "{debug}"
        );

        // --debug but no/blank id -> still no Request ID line.
        let none = format_api_error(
            "domain purchase",
            422,
            "422 Unprocessable Entity",
            "x",
            None,
            true,
        );
        assert!(!none.contains("Request ID"), "{none}");
        let blank = format_api_error(
            "domain purchase",
            422,
            "422 Unprocessable Entity",
            "x",
            Some("  "),
            true,
        );
        assert!(!blank.contains("Request ID"), "{blank}");
    }

    #[test]
    fn customer_id_strips_customer_urn_prefix_from_sub() {
        // GoDaddy's `sub` is a typed subject URN; the v2 path needs the bare uuid.
        let cred = Credential {
            sub: "customer:56fd82e4-1c45-4596-865d-317235015b2f".to_string(),
            ..Default::default()
        };
        assert_eq!(
            customer_id(&cred).expect("customer subject"),
            "56fd82e4-1c45-4596-865d-317235015b2f"
        );

        // A non-customer subject (or empty) isn't a customer id → clear error.
        let shopper = Credential {
            sub: "shopper:12345".to_string(),
            ..Default::default()
        };
        let err = customer_id(&shopper).expect_err("not a customer subject");
        assert!(err.to_string().contains("not a customer identity"), "{err}");
        assert!(customer_id(&Credential::default()).is_err());
    }

    #[test]
    fn agreed_at_is_zulu_iso_datetime_not_offset() {
        use chrono::TimeZone;
        let dt = chrono::Utc
            .with_ymd_and_hms(2026, 6, 17, 22, 34, 43)
            .single()
            .expect("valid instant");
        let s = iso_datetime(dt);
        // The API's iso-datetime pattern requires a trailing `Z` and rejects the
        // `+00:00` offset form `chrono::to_rfc3339()` would produce.
        assert_eq!(s, "2026-06-17T22:34:43Z");
        assert!(!s.contains('+'), "must not use a numeric offset: {s}");
    }

    #[test]
    fn tld_preflight_blocks_missing_required_contact() {
        // .fun requires a registrant; we're sending no contacts -> block with an
        // actionable message before the paid call.
        let required = vec![
            "domain".to_string(),
            "consent".to_string(),
            "contactRegistrant".to_string(),
        ];
        let err = check_tld_requirements("fun", &required, &[]).expect_err("registrant required");
        let msg = err.to_string();
        assert!(msg.contains("registrant"), "{msg}");
        assert!(msg.contains("contacts.toml"), "{msg}");

        // Sending a registrant satisfies it.
        check_tld_requirements("fun", &required, &[Role::Registrant])
            .expect("registrant present satisfies the requirement");
    }

    #[test]
    fn tld_preflight_allows_only_always_sent_fields() {
        let required = vec![
            "domain".to_string(),
            "consent".to_string(),
            "period".to_string(),
            "privacy".to_string(),
            "renewAuto".to_string(),
        ];
        check_tld_requirements("com", &required, &[]).expect("always-sent fields never block");
    }

    #[test]
    fn tld_preflight_blocks_unsupplyable_field() {
        // A required field outside DomainPurchase -> block and point at the
        // schema command (rather than attempt a guaranteed-422 purchase).
        let required = vec!["domain".to_string(), "registrySpecificThing".to_string()];
        let err =
            check_tld_requirements("example", &required, &[]).expect_err("unknown field blocks");
        let msg = err.to_string();
        assert!(msg.contains("registrySpecificThing"), "{msg}");
        assert!(msg.contains("domain schema"), "{msg}");
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
        let cases: [&[&str]; 7] = [
            &["gddy", "domain", "list", "--output", "json"],
            &["gddy", "domain", "get", "example.com", "--output", "json"],
            &[
                "gddy",
                "domain",
                "available",
                "example.com",
                "--output",
                "json",
            ],
            &["gddy", "domain", "suggest", "coffee", "--output", "json"],
            &[
                "gddy",
                "domain",
                "agreements",
                "--tld",
                "com",
                "--output",
                "json",
            ],
            &["gddy", "domain", "schema", "fun", "--output", "json"],
            // --agree/--confirm present so this would reach the handler if it
            // weren't fail-closed: auth must still reject it first.
            &[
                "gddy",
                "domain",
                "purchase",
                "example.com",
                "--agree",
                "--confirm",
                "--output",
                "json",
            ],
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
