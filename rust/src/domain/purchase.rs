//! `gddy domain purchase` — register a domain by accepting a cached quote (v3).

use cli_engine::{
    CliCoreError, CommandResult, CommandSpec, Credential, NextAction, NextActionParam, Result,
    RuntimeCommandSpec, Tier,
};
use serde_json::json;

use domains_client::types;

use super::common::{api_error, make_client_with_cred};
use crate::auth::SSO_KEY_PROVIDER;
use crate::output_schema::output_schema;
use crate::quote_cache;
use crate::scopes::{DOMAINS_CREATE, DOMAINS_READ};

output_schema!(DomainPurchaseResult {
    "domain": "string";
    "status": "string";
    // Present depending on the async operation + the cached quote's receipt.
    "operationId": "string", optional;
    "price": "string", optional;
    "currency": "string", optional;
});

/// Format a UTC instant as the API's consent timestamp: RFC 3339 with a literal
/// trailing `Z` (e.g. `2026-06-30T22:34:43Z`), sub-second digits dropped.
fn iso_datetime(now: chrono::DateTime<chrono::Utc>) -> String {
    now.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// The GoDaddy account/customer id used as the consent `principal`, taken from the
/// OAuth token's typed `sub` claim (`customer:<uuid>`). The v3 register endpoint
/// verifies this principal against the authenticated identity, so a non-customer
/// subject is rejected with a clear error before the paid call.
fn consent_principal(cred: &Credential) -> Result<String> {
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

/// Whether an async domain-operation status is terminal (no further polling).
/// The non-terminal states are `CONFIRMED`/`EXECUTING`; `COMPLETED`/`FAILED` end it.
fn is_terminal_status(status: &str) -> bool {
    matches!(status, "COMPLETED" | "FAILED")
}

/// Enforce the purchase gates against a cached quote, returning the agreement
/// types to record as consent. Pure (no I/O) so the gating is unit-testable:
/// `--agree` is the legal-consent gate (its error lists the agreements — by their
/// human titles from the quote — to review); `--confirm` is the charge gate (the
/// registration is paid). The agreement types are only returned once both gates
/// pass. `agreement_titles`/`agreement_types` come from the cached quote (the
/// types are what the register call must echo into `consent.agreementTypes`).
fn purchase_consent_types(
    domain: &str,
    period: u64,
    agree: bool,
    confirm: bool,
    agreement_titles: &[String],
    agreement_types: &[String],
) -> Result<Vec<types::AgreementType>> {
    // Validate the cached quote actually carries agreement types *before* the
    // --agree gate: an empty list means a corrupt/outdated cache the command can
    // never proceed with, so surface that single accurate error rather than a
    // "requires agreeing…" prompt with an empty list that a rerun can't satisfy.
    if agreement_types.is_empty() {
        return Err(CliCoreError::message(format!(
            "the cached quote for {domain} recorded no legal agreement types (the cache is \
             corrupt or from an older CLI version); re-run `gddy domain quote {domain}` for a \
             fresh quote."
        )));
    }

    if !agree {
        let list = agreement_titles
            .iter()
            .map(|t| format!("  - {t}"))
            .collect::<Vec<_>>()
            .join("\n");
        return Err(CliCoreError::message(format!(
            "registering {domain} requires agreeing to its legal agreement(s):\n{list}\n\n\
             Re-run with --agree to accept them. See `gddy guide domain-purchase`."
        )));
    }

    if !confirm {
        return Err(CliCoreError::message(format!(
            "registering {domain} for {period} year(s) charges your account and cannot be undone; \
             re-run with --confirm to proceed"
        )));
    }

    Ok(agreement_types
        .iter()
        .map(|t| types::AgreementType(t.clone()))
        .collect())
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_with_context(
        CommandSpec::new(
            "purchase",
            "Register a domain from a quote (paid; charges your account)",
        )
        .with_long(
            "Register a domain by accepting a quote from `gddy domain quote`. This \
                 charges your GoDaddy account and cannot be undone, so it is gated behind \
                 --confirm.\n\
                 \n\
                 Registration settings (period, privacy, nameservers, contacts) are fixed \
                 at quote time — the quote token locks both those settings and the price. \
                 `purchase` accepts the token, records your consent to the quote's legal \
                 agreements (--agree), then registers and waits for the registry to \
                 finish. A usable payment method must be on file — add one with \
                 `gddy payments add`.\n\
                 \n\
                 Typical flow:\n  \
                 1. gddy domain quote example.com          # review price + agreements\n  \
                 2. gddy domain purchase --quote-token <token> --agree --confirm\n\
                 \n\
                 The quote is cached locally, so run both on the same machine within the \
                 token's ~10-minute lifetime. See `gddy guide domain-purchase`.",
        )
        .with_system("domain")
        .with_tier(Tier::Destructive)
        .with_default_fields("domain,status,operationId,price,currency")
        .with_output_schema::<DomainPurchaseResult>()
        .with_scopes(&[DOMAINS_READ, DOMAINS_CREATE])
        .with_arg(
            clap::Arg::new("quote-token")
                .long("quote-token")
                .value_name("TOKEN")
                .required(true)
                .help("The quote token from `gddy domain quote` (locks the price + settings)"),
        )
        .with_arg(
            clap::Arg::new("agree")
                .long("agree")
                .action(clap::ArgAction::SetTrue)
                .help(
                    "Consent to the quote's legal agreements (run without it to list \
                             them; review with `gddy guide domain-purchase`)",
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
            let quote_token = ctx
                .args
                .get("quote-token")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_owned();
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

            // The v3 register endpoint needs the customer identity from an OAuth
            // token (for the consent principal); sso-key auth can never satisfy
            // it. Resolve auth *before* consuming the cached quote so a failed
            // login never touches the cache.
            let cred = ctx.credential().await?;
            if cred.provider == SSO_KEY_PROVIDER {
                return Err(CliCoreError::message(
                    "`domain purchase` uses the v3 registration API, which requires OAuth \
                         authentication; this environment is configured for an sso-key. Re-run \
                         against an OAuth environment.",
                ));
            }
            let principal = consent_principal(&cred)?;

            // Load the quote the user reviewed. Read-only: the entry is only
            // removed once the registration succeeds, so an un-`--agree`d run or
            // a failed charge leaves the quote reusable.
            let cached = match quote_cache::get(&quote_token) {
                quote_cache::Lookup::Found(q) => *q,
                quote_cache::Lookup::Expired => {
                    return Err(CliCoreError::message(
                        "that quote has expired (quotes last ~10 minutes). Re-run \
                             `gddy domain quote <domain>` for a fresh quote and token.",
                    ));
                }
                quote_cache::Lookup::Missing => {
                    return Err(CliCoreError::message(
                        "no cached quote for that token. Run `gddy domain quote <domain>` \
                             first — quotes are cached locally, so quote and purchase must run \
                             on the same machine within the token's ~10-minute lifetime.",
                    ));
                }
            };
            let domain = cached.domain.clone();

            let agreement_types = purchase_consent_types(
                &domain,
                cached.period,
                agree,
                confirm,
                &cached.agreement_titles,
                &cached.agreement_types,
            )?;
            let period_nz = std::num::NonZeroU64::new(cached.period).ok_or_else(|| {
                CliCoreError::message("the cached quote has an invalid registration period")
            })?;

            // Fail fast if a cached profile won't deserialize: silently dropping
            // it would re-send a *different* request than was quoted and surface
            // as a confusing server-side QUOTE_MISMATCH. A clear re-quote message
            // is better than a mismatched charge attempt.
            let profile = match cached.profile.as_ref() {
                Some(v) => Some(
                    serde_json::from_value::<types::InlineRegistrationProfile>(v.clone()).map_err(
                        |e| {
                            CliCoreError::message(format!(
                                "the cached quote for {domain} is corrupt or from an older CLI \
                                 version (could not read its registration profile: {e}); re-run \
                                 `gddy domain quote {domain}` for a fresh quote."
                            ))
                        },
                    )?,
                ),
                None => None,
            };

            let consent = types::Consent {
                agreed_at: types::DateTime(iso_datetime(chrono::Utc::now())),
                agreed_by: types::ConsentActor {
                    actor: None,
                    ip: Some(agreed_by),
                    principal,
                    type_: types::ConsentActorType("DIRECT".to_string()),
                },
                agreement_types,
            };
            let registration = types::Registration {
                consent,
                created_at: None,
                domain: domain.clone(),
                expires_at: None,
                links: vec![],
                operation_id: None,
                period: period_nz,
                profile,
                profile_id: None,
                quote_token: Some(types::Uuid(quote_token.clone())),
                registration_id: None,
                status: None,
                updated_at: None,
            };

            let client = make_client_with_cred(&ctx.middleware.env, &cred)?;
            let idempotency_key = uuid::Uuid::new_v4().to_string();
            let accepted = match client
                .register_domain()
                .idempotency_key(idempotency_key)
                .body(registration)
                .send()
                .await
            {
                Ok(r) => r.into_inner(),
                Err(e) => return Err(api_error("domain purchase", debug, e).await),
            };
            // The token is consumed server-side on a successful execute, so drop
            // our cached copy too (single-use).
            quote_cache::remove(&quote_token);
            let price = cached.price.clone();
            let currency = cached.currency.clone();

            // Poll the async operation to a terminal state (best-effort, bounded).
            let mut status = accepted
                .status
                .as_ref()
                .map(|s| s.to_string())
                .unwrap_or_else(|| "SUBMITTED".to_string());
            let operation_id = accepted.operation_id.clone();
            if let Some(op_id) = operation_id.as_ref() {
                for _ in 0..20 {
                    if is_terminal_status(&status) {
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                    match client
                        .get_operation()
                        .operation_id(op_id.clone())
                        .send()
                        .await
                    {
                        Ok(r) => {
                            if let Some(s) = r.into_inner().status {
                                status = s.to_string();
                            }
                        }
                        // A transient poll failure shouldn't fail the purchase —
                        // it already succeeded; report the last-known status.
                        Err(_) => break,
                    }
                }
            }

            let result = json!({
                "domain": domain,
                "status": status,
                "operationId": operation_id.map(|o| o.to_string()),
                "price": price,
                "currency": currency,
            });
            Ok(CommandResult::new(result).with_next_actions(vec![
                NextAction::new("domain get <domain>", "See the registered domain's details")
                    .with_param("domain", NextActionParam::required()),
            ]))
        },
    )
}

#[cfg(test)]
mod tests {
    use super::{consent_principal, is_terminal_status, iso_datetime, purchase_consent_types};
    use cli_engine::Credential;

    #[test]
    fn purchase_consent_requires_agree_then_confirm() {
        // Titles + types as they'd come from a cached quote.
        let titles = vec!["Registration Agreement (https://x)".to_string()];
        let ty = vec!["DNRA".to_string()];

        let err = purchase_consent_types("example.com", 1, false, true, &titles, &ty)
            .expect_err("must require --agree");
        let msg = err.to_string();
        assert!(msg.contains("Registration Agreement"), "{msg}");
        assert!(msg.contains("--agree"), "{msg}");

        let err = purchase_consent_types("example.com", 2, true, false, &titles, &ty)
            .expect_err("must require --confirm");
        let msg = err.to_string();
        assert!(msg.contains("--confirm"), "{msg}");
        assert!(msg.contains("2 year(s)"), "{msg}");

        let types = purchase_consent_types("example.com", 1, true, true, &titles, &ty)
            .expect("both gates satisfied");
        assert_eq!(types.len(), 1);
        assert_eq!(types[0].as_str(), "DNRA");
    }

    #[test]
    fn purchase_consent_rejects_when_no_agreement_types() {
        // --agree given, but the cached quote carried no agreement types.
        let err = purchase_consent_types("example.com", 1, true, true, &[], &[])
            .expect_err("no types -> error");
        assert!(
            err.to_string().contains("no legal agreement types"),
            "{err}"
        );
    }

    #[test]
    fn empty_agreement_types_is_reported_before_the_agree_gate() {
        // Without --agree AND with no cached agreement types, the accurate
        // "no agreement types / re-quote" error must win over the "requires
        // agreeing…" prompt (which would list nothing and never be satisfiable).
        let err = purchase_consent_types("example.com", 1, false, false, &[], &[])
            .expect_err("empty types -> error");
        let msg = err.to_string();
        assert!(msg.contains("no legal agreement types"), "{msg}");
        assert!(!msg.contains("requires agreeing"), "{msg}");
    }

    #[test]
    fn terminal_status_detection() {
        assert!(is_terminal_status("COMPLETED"));
        assert!(is_terminal_status("FAILED"));
        assert!(!is_terminal_status("CONFIRMED"));
        assert!(!is_terminal_status("EXECUTING"));
        assert!(!is_terminal_status("SUBMITTED"));
    }

    #[test]
    fn consent_principal_strips_customer_urn_prefix() {
        let cred = Credential {
            sub: "customer:56fd82e4-1c45-4596-865d-317235015b2f".to_string(),
            ..Default::default()
        };
        assert_eq!(
            consent_principal(&cred).expect("customer subject"),
            "56fd82e4-1c45-4596-865d-317235015b2f"
        );
        let shopper = Credential {
            sub: "shopper:12345".to_string(),
            ..Default::default()
        };
        assert!(consent_principal(&shopper).is_err());
    }

    #[test]
    fn agreed_at_is_zulu_iso_datetime_not_offset() {
        use chrono::TimeZone;
        let dt = chrono::Utc
            .with_ymd_and_hms(2026, 6, 30, 22, 34, 43)
            .single()
            .expect("valid instant");
        let s = iso_datetime(dt);
        assert_eq!(s, "2026-06-30T22:34:43Z");
        assert!(!s.contains('+'), "must not use a numeric offset: {s}");
    }
}
