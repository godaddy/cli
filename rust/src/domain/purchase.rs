//! `gddy domain purchase` — register a domain by accepting a cached quote (v3).

use cli_engine::{
    CliCoreError, CommandResult, CommandSpec, Credential, NextActionParam, Result,
    RuntimeCommandSpec, Tier,
};
use serde_json::json;

use domains_client::types;

use super::common::{api_error, format_operation_error, is_terminal_status, make_client_with_cred};
use crate::next_action::next_action;
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

/// Validate the OAuth token is a customer identity (`customer:<uuid>`), so a
/// non-customer subject is rejected with a clear local error *before* the paid
/// call and before the cached quote is consumed. `agreedBy` is server-derived
/// from the request's auth context (the API no longer accepts a caller-supplied
/// consent principal), so the returned id isn't sent anywhere — this is purely a
/// fail-fast check.
fn consent_principal(cred: &Credential) -> Result<String> {
    let id = cred
        .sub
        .strip_prefix("customer:")
        .filter(|id| !id.is_empty())
        .ok_or_else(|| {
            CliCoreError::message(format!(
                "the OAuth token's subject ({:?}) is not a customer identity; `domain purchase` \
                 needs a customer-scoped token",
                cred.sub
            ))
        })?;
    // Validate it's a customer UUID up front so a malformed
    // `customer:<not-a-uuid>` subject fails fast with a clear message rather
    // than as an opaque server-side rejection.
    if uuid::Uuid::parse_str(id).is_err() {
        return Err(CliCoreError::message(format!(
            "the OAuth token's customer subject ({:?}) is not a valid UUID; `domain purchase` \
             needs a customer-scoped token",
            cred.sub
        )));
    }
    Ok(id.to_owned())
}

/// Description for the `domain get <domain>` next-action, tailored to whether
/// registration actually finished (`COMPLETED`) or bounded polling gave up
/// while it was still non-terminal — the latter shouldn't claim the domain is
/// already registered when it may not be provisioned yet.
fn next_action_description(status: &str) -> &'static str {
    if status == "COMPLETED" {
        "See the registered domain's details"
    } else {
        "Check whether registration has finished (was still in progress after polling)"
    }
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
        // Prefer the human titles, but fall back to the agreement *types* when a
        // quote from an older CLI cached no titles (they're `#[serde(default)]`) —
        // types are guaranteed non-empty here, so the list is always actionable.
        let items = if agreement_titles.is_empty() {
            agreement_types
        } else {
            agreement_titles
        };
        let list = items
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
        .cloned()
        .map(types::AgreementType)
        .collect())
}

#[derive(Debug, Clone, clap::Args)]
struct PurchaseArgs {
    /// The quote token from `gddy domain quote` (locks the price + settings).
    #[arg(long = "quote-token", value_name = "TOKEN")]
    quote_token: String,

    /// Consent to the quote's legal agreements (run without it to list them;
    /// review with `gddy guide domain-purchase`).
    #[arg(long)]
    agree: bool,

    /// Confirm the purchase; required because it charges your account.
    #[arg(long)]
    confirm: bool,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<PurchaseArgs, _, _, _>(
        CommandSpec::from_args::<PurchaseArgs>(
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
                 `gddy payment-methods add`.\n\
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
        .with_scopes(&[DOMAINS_READ, DOMAINS_CREATE]),
        |ctx, args: PurchaseArgs| async move {
            let quote_token = args.quote_token;
            let agree = args.agree;
            let confirm = args.confirm;
            let debug = !ctx.middleware.debug.is_empty();

            // Resolve auth *before* consuming the cached quote, so a token that
            // isn't customer-scoped fails locally rather than after the cache
            // entry (and the ~10-minute quote window) is spent. The register
            // endpoint derives `agreedBy` server-side, so only the validation
            // (the early return on error) is needed here — the customer id
            // itself is intentionally discarded.
            let cred = ctx.credential().await?;
            let _customer_id = consent_principal(&cred)?;

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
                quote_cache::Lookup::NoConfigDir => {
                    return Err(CliCoreError::message(
                        "could not locate a config directory to read the quote cache from. \
                             `domain purchase` needs the local quote written by `domain quote`; \
                             ensure a home/config directory is available (e.g. set HOME or \
                             XDG_CONFIG_HOME) and re-run `gddy domain quote <domain>`.",
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

            // Echo the quote's fees (e.g. a premium domain's one-time
            // acquisition surcharge) back verbatim into `consent.acknowledgedFees`
            // — the server cross-checks this against the locked quote token and
            // rejects a mismatch with `422 quote_mismatch`. `None`/absent here
            // means the quote carried no fees (the common, non-premium case),
            // which correctly serializes as no fees to acknowledge.
            let acknowledged_fees = match cached.fees.as_ref() {
                Some(v) => serde_json::from_value::<Vec<types::Fee>>(v.clone()).map_err(|e| {
                    CliCoreError::message(format!(
                        "the cached quote for {domain} is corrupt or from an older CLI \
                         version (could not read its fees: {e}); re-run \
                         `gddy domain quote {domain}` for a fresh quote."
                    ))
                })?,
                None => vec![],
            };

            let consent = types::Consent {
                agreed_at: types::DateTime(iso_datetime(chrono::Utc::now())),
                // Server-derived from the execute request's auth context; the
                // caller no longer supplies this.
                agreed_by: None,
                agreement_types,
                acknowledged_fees,
            };
            let registration = types::Registration {
                consent,
                created_at: None,
                domain: domain.clone(),
                expires_at: None,
                // Server-populated on the response; not sent in the request.
                fees: vec![],
                links: vec![],
                operation_id: None,
                order_id: None,
                period: period_nz,
                price: None,
                profile,
                profile_id: None,
                quote_token: Some(types::Uuid(quote_token.clone())),
                registration_id: None,
                status: None,
                updated_at: None,
            };

            let client = make_client_with_cred(&ctx.middleware.env, &cred)?;
            // Reuse the quote's idempotency key across attempts (older cache
            // entries may lack one — fall back to a fresh key).
            let idempotency_key = cached
                .idempotency_key
                .clone()
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
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
            let mut operation_error: Option<types::Error> = None;
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
                            let op = r.into_inner();
                            if let Some(s) = op.status {
                                status = s.to_string();
                            }
                            operation_error = op.error;
                        }
                        // A transient poll failure shouldn't fail the purchase —
                        // it already succeeded; report the last-known status.
                        Err(_) => break,
                    }
                }
            }

            // A terminal FAILED status means the registration did not complete —
            // report it as an error (non-zero exit), not a success payload. No
            // domain was ever registered, so don't point at `domain get` (it
            // won't find anything) — the only viable next step is a fresh quote.
            if status == "FAILED" {
                let op = operation_id
                    .as_ref()
                    .map(|o| o.to_string())
                    .unwrap_or_else(|| "unknown".to_string());
                let detail = format_operation_error(operation_error.as_ref());
                return Err(CliCoreError::message(format!(
                    "registration for {domain} failed (operation {op}){detail}; no domain was \
                     registered. Re-run `gddy domain quote {domain}` for a fresh quote and try again."
                )));
            }
            // Bounded polling gave up before a terminal state (e.g. still
            // SUBMITTED/PENDING). It may still complete server-side, so tell the
            // user how to check rather than implying it finished.
            let still_pending = operation_id.is_some() && !is_terminal_status(&status);
            if let Some(op) = operation_id.as_ref().filter(|_| still_pending) {
                tracing::info!(
                    %domain,
                    %status,
                    operation_id = %op,
                    "registration still in progress after polling; check later with \
                     `gddy domain operation status {op}`"
                );
            }

            // Only emit the optional fields when present — never JSON null (the
            // schema marks them optional strings, and null leaks into tables).
            let mut result = json!({
                "domain": domain,
                "status": status,
            });
            if let Some(op) = operation_id.as_ref() {
                result["operationId"] = json!(op.to_string());
            }
            if let Some(p) = price.as_ref() {
                result["price"] = json!(p);
            }
            if let Some(c) = currency.as_ref() {
                result["currency"] = json!(c);
            }
            let mut actions = vec![
                next_action("domain get <domain>", next_action_description(&status))
                    .with_param("domain", NextActionParam::required()),
            ];
            if still_pending {
                // `still_pending` implies `operation_id.is_some()`.
                if let Some(op) = &operation_id {
                    actions.push(
                        next_action(
                            "domain operation status <operation-id>",
                            "Check whether registration has finished since polling gave up",
                        )
                        .with_param("operation-id", NextActionParam::value(op.to_string())),
                    );
                }
            }
            Ok(CommandResult::new(result).with_next_actions(actions))
        },
    )
}

#[cfg(test)]
mod tests {
    use super::{consent_principal, iso_datetime, next_action_description, purchase_consent_types};
    use cli_engine::Credential;
    use domains_client::types;

    #[test]
    fn purchase_consent_requires_agree_then_confirm() {
        // Titles + types as they'd come from a cached quote.
        let titles = vec!["Registration Agreement (https://x)".to_string()];
        let ty = vec!["API_DPA".to_string()];

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
        assert_eq!(types[0].to_string(), "API_DPA");
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
    fn agree_gate_lists_types_when_titles_absent() {
        // An older cached quote may carry agreement types but no human titles
        // (titles are serde-default). The --agree prompt must still list
        // something actionable — fall back to the types.
        let err =
            purchase_consent_types("example.com", 1, false, true, &[], &["API_DPA".to_string()])
                .expect_err("must require --agree");
        let msg = err.to_string();
        assert!(msg.contains("requires agreeing"), "{msg}");
        assert!(msg.contains("- API_DPA"), "{msg}");
    }

    #[test]
    fn next_action_description_reflects_final_status() {
        // A true COMPLETED gets the "already registered" wording.
        assert_eq!(
            next_action_description("COMPLETED"),
            "See the registered domain's details"
        );
        // Any non-terminal status left over after polling gave up must NOT
        // claim the domain is already registered.
        for status in ["EXECUTING", "SUBMITTED", "CONFIRMED"] {
            let desc = next_action_description(status);
            assert!(
                !desc.contains("registered domain"),
                "status {status:?} must not claim the domain is registered: {desc}"
            );
        }
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

        // A customer subject that isn't a UUID must fail fast before the paid call.
        let not_uuid = Credential {
            sub: "customer:12345".to_string(),
            ..Default::default()
        };
        let err = consent_principal(&not_uuid).expect_err("non-uuid customer subject");
        assert!(err.to_string().contains("not a valid UUID"), "{err}");
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

    #[test]
    fn empty_optional_vecs_are_omitted_not_sent_as_empty_arrays() {
        // `Consent.acknowledgedFees` (minItems: 1 when present, "omit when
        // the quote carries no purchase fees") and `Registration.fees`
        // (readOnly) must never be serialized as `[]` for the common
        // non-premium case — that would violate the schema and risk
        // rejection. Pins the `skip_serializing_if` progenitor generates for
        // these fields, since this code relies on it by constructing both
        // with `vec![]` rather than omitting the field outright.
        let consent = types::Consent {
            agreed_at: types::DateTime("2026-06-30T00:00:00Z".to_string()),
            agreed_by: None,
            agreement_types: vec![types::AgreementType("API_DPA".to_string())],
            acknowledged_fees: vec![],
        };
        let value = serde_json::to_value(&consent).expect("serializes");
        assert!(
            !value
                .as_object()
                .expect("object")
                .contains_key("acknowledgedFees"),
            "{value}"
        );

        let registration = types::Registration {
            consent,
            created_at: None,
            domain: "example.com".to_string(),
            expires_at: None,
            fees: vec![],
            links: vec![],
            operation_id: None,
            order_id: None,
            period: std::num::NonZeroU64::new(1).expect("nonzero"),
            price: None,
            profile: None,
            profile_id: None,
            quote_token: Some(types::Uuid("tok-abc".to_string())),
            registration_id: None,
            status: None,
            updated_at: None,
        };
        let value = serde_json::to_value(&registration).expect("serializes");
        assert!(
            !value.as_object().expect("object").contains_key("fees"),
            "{value}"
        );
    }
}
