//! Step 4: Execute — submit the registration using the cached quote, poll the
//! async operation, and display the result.

use cli_engine::{CliCoreError, Result};
use console::style;
use indicatif::{ProgressBar, ProgressStyle};

use domains_client::types;

use crate::domain::common::{
    api_error, format_operation_error, is_terminal_status, make_client_with_cred,
};
use crate::quote_cache;

use super::super::retry::with_retry;
use super::super::wizard::{StepContext, StepResult, WizardState};

pub(crate) async fn run(state: &mut WizardState, ctx: &StepContext) -> Result<StepResult> {
    let domain = state
        .domain
        .as_ref()
        .ok_or_else(|| CliCoreError::message("no domain selected"))?
        .clone();
    let quote_token = state
        .quote_token
        .as_ref()
        .ok_or_else(|| CliCoreError::message("no quote token available"))?
        .clone();

    let client = make_client_with_cred(&ctx.env, &ctx.credential)?;
    let debug = ctx.debug;

    // Validate credential is a customer identity.
    let _customer_id = ctx
        .credential
        .sub
        .strip_prefix("customer:")
        .filter(|id| !id.is_empty())
        .ok_or_else(|| {
            CliCoreError::message(format!(
                "the OAuth token's subject ({:?}) is not a customer identity; \
                 domain registration needs a customer-scoped token",
                ctx.credential.sub
            ))
        })?;

    // Build consent.
    let agreement_types: Vec<types::AgreementType> = state
        .agreement_types
        .iter()
        .map(|t| {
            t.parse::<types::AgreementType>().map_err(|_| {
                CliCoreError::message(format!(
                    "unrecognized agreement type ({t:?}); re-run the wizard for a fresh quote"
                ))
            })
        })
        .collect::<Result<Vec<_>>>()?;

    let period_nz = std::num::NonZeroU64::new(state.period)
        .ok_or_else(|| CliCoreError::message("invalid registration period"))?;

    let (profile, acknowledged_fees) = match quote_cache::get(&quote_token) {
        quote_cache::Lookup::Found(cached) => {
            let prof = cached
                .profile
                .as_ref()
                .map(|v| serde_json::from_value::<types::InlineRegistrationProfile>(v.clone()))
                .transpose()
                .map_err(|e| {
                    CliCoreError::message(format!("corrupt cached profile: {e}; re-run the wizard"))
                })?;
            let fees = match cached.fees.as_ref() {
                Some(v) => serde_json::from_value::<Vec<types::Fee>>(v.clone()).map_err(|e| {
                    CliCoreError::message(format!(
                        "the cached quote is corrupt or from an older CLI version \
                         (could not read its fees: {e}); re-run the wizard for a fresh quote."
                    ))
                })?,
                None => vec![],
            };
            (prof, fees)
        }
        _ => (None, vec![]),
    };

    let consent = types::Consent {
        agreed_at: types::DateTime(
            chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        ),
        agreed_by: None,
        agreement_types,
        acknowledged_fees,
    };

    let registration = types::Registration {
        consent,
        created_at: None,
        domain: domain.clone(),
        expires_at: None,
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

    // Show spinner during registration.
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .template("  {spinner} {msg}")
            .expect("valid template"),
    );
    spinner.set_message(format!("Registering {}...", &domain));
    spinner.enable_steady_tick(std::time::Duration::from_millis(100));

    let idempotency_key = uuid::Uuid::new_v4().to_string();
    let accepted = match with_retry("registration", 3, || {
        let c = &client;
        let key = &idempotency_key;
        let reg = registration.clone();
        async move {
            c.register_domain()
                .idempotency_key(key)
                .body(reg)
                .send()
                .await
        }
    })
    .await
    {
        Ok(r) => r.into_inner(),
        Err(e) => {
            spinner.finish_and_clear();
            return Err(api_error("domain register", debug, e).await);
        }
    };

    // Consume the quote token.
    quote_cache::remove(&quote_token);

    // Poll operation to terminal state.
    let mut status = accepted
        .status
        .as_ref()
        .map(|s| s.to_string())
        .unwrap_or_else(|| "SUBMITTED".to_string());
    let operation_id = accepted.operation_id.clone();
    let mut operation_error: Option<types::Error> = None;

    if let Some(op_id) = operation_id.as_ref() {
        spinner.set_message(format!("Waiting for registry ({})", &domain));
        let mut timed_out = false;
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
                Err(_) => break,
            }
        }
        if !is_terminal_status(&status) {
            timed_out = true;
        }
        if timed_out {
            spinner.finish_and_clear();
            eprintln!(
                "\n  {} The registration was submitted successfully but the registry hasn't \
                 confirmed yet.",
                style("⏳").bold()
            );
            eprintln!("     Your domain will be registered — this is normal for some TLDs.");
            eprintln!(
                "     Check progress with: gddy domain operation status {}",
                op_id
            );
        }
    }

    spinner.finish_and_clear();

    // Display result.
    if status == "FAILED" {
        let detail = format_operation_error(operation_error.as_ref());
        return Err(CliCoreError::message(format!(
            "registration for {domain} failed{detail}; no domain was registered. \
             Please try again."
        )));
    }

    if status == "COMPLETED" {
        eprintln!(
            "\n  {} {} has been registered!",
            style("🎉").bold(),
            style(&domain).green().bold()
        );
    } else if !is_terminal_status(&status) {
        // Already printed timeout message above; skip duplicate output.
    } else {
        eprintln!(
            "\n  {} Registration submitted for {} (status: {})",
            style("⏳").bold(),
            style(&domain).cyan(),
            status
        );
        if let Some(op) = &operation_id {
            eprintln!(
                "     Check progress with: gddy domain operation status {}",
                op
            );
        }
    }

    if let Some(price) = &state.price {
        let currency = state.currency.as_deref().unwrap_or("");
        eprintln!("     Charged: {} {}", price, currency);
    }

    eprintln!("\n  Next steps:");
    eprintln!("    • gddy domain get {domain}");
    eprintln!("    • gddy dns set {domain} --type A --name @ --data <ip>");

    state.status = Some(status);
    state.operation_id = operation_id.map(|o| o.to_string());

    Ok(StepResult::Continue)
}
