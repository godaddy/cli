use cli_engine::{
    CommandSpec, NextAction, NextActionParam, RuntimeCommandSpec, StreamSender, Tier,
};
use email_client::types;
use serde_json::{Value, json};

use crate::email::client::{ClientError, EmailClient};
use crate::email::terms_acceptance;
use crate::email::{body_has_issue, client_err, client_err_with_fix, make_client};
use crate::next_action::next_action;
use crate::scopes::EMAIL_READ;

#[derive(Debug, Clone, clap::Args)]
struct CheckEligibilityArgs {
    #[arg(long, value_name = "EMAIL")]
    email: String,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_streaming::<CheckEligibilityArgs, _, _>(
        CommandSpec::from_args::<CheckEligibilityArgs>(
            "check-eligibility",
            "Check whether an email address is eligible for a new mailbox",
        )
        .with_system("email")
        .with_tier(Tier::Read)
        .with_scopes(&[EMAIL_READ]),
        |ctx, args: CheckEligibilityArgs, sender: StreamSender| async move {
            let client = match make_client(&ctx, &[EMAIL_READ]).await {
                Ok(c) => c,
                Err(e) => return Err(fail_check(&sender, e).await),
            };

            let data = match run_eligibility_with_mca(&client, &args.email, &sender).await {
                Ok((d, _)) => d,
                Err(e) => return Err(fail_check(&sender, e).await),
            };

            let next_actions = eligibility_next_actions(&args.email, &data);
            sender
                .send(json!({
                    "type": "result",
                    "ok": true,
                    "result": data,
                    "next_actions": next_actions,
                }))
                .await;
            Ok(())
        },
    )
}

/// Runs the eligibility check, automatically handling Terms & Conditions
/// acceptance if the API requires it.
///
/// When any eligible account carries a `MICROSOFT_MCA` requirement:
/// 1. [`terms_acceptance::wait_for_acceptance`] opens the browser, streams
///    progress events, and polls until the user clicks Accept.
/// 2. The eligibility check is retried automatically.
///
/// Returns `(eligibility_data, accepted_requirement_types)` where
/// `accepted_requirement_types` lists non-MCA requirement `type` strings that
/// were accepted interactively. `MICROSOFT_MCA` is intentionally excluded —
/// acceptance is recorded via browser attestation, not a consent payload flag.
///
/// All other errors are returned immediately.
pub(super) async fn run_eligibility_with_mca(
    client: &EmailClient,
    email: &str,
    sender: &StreamSender,
) -> cli_engine::Result<(types::EligibilityResult, Vec<String>)> {
    match client.check_eligibility(email).await {
        Ok(data) => {
            let mca_req = data
                .eligible_accounts
                .iter()
                .find_map(terms_acceptance::extract_mca_requirement);
            if let Some(req) = mca_req {
                terms_acceptance::wait_for_acceptance(&req, client, sender).await?;
                // Don't re-check eligibility — the backend may not immediately
                // reflect the acceptance. MCA is recorded via attestation URL,
                // not a consent payload flag, so accepted_types stays empty.
                Ok((data, Vec::new()))
            } else {
                Ok((data, Vec::new()))
            }
        }
        Err(ClientError::Http { status: 422, body })
            if body_has_issue(&body, "NEEDS_TITAN_PLAN")
                || body_has_issue(&body, "NEEDS_MICROSOFT_PLAN")
                || body_has_issue(&body, "EMAIL_PLAN_NOT_AVAILABLE") =>
        {
            let fix = if body_has_issue(&body, "NEEDS_MICROSOFT_PLAN") {
                "Run: 'gddy shopping catalog search --query microsoft' \
                 to see available Microsoft 365 email plans, select one to purchase, then retry."
            } else {
                "Run: 'gddy shopping catalog search --query titan' \
                 to see available email plans, select one to purchase, then retry."
            };
            Err(client_err_with_fix(
                ClientError::Http { status: 422, body },
                fix,
            ))
        }
        Err(e) => Err(client_err(e)),
    }
}

/// Builds the terminal `{"type":"error",...}` event for streaming output,
/// using `cli_engine::build_error_envelope` so the `code`/`message`/`fix`
/// match what a non-streaming command would have rendered.
fn check_error_event(err: &cli_engine::CliCoreError) -> Value {
    let envelope = cli_engine::build_error_envelope(err, "email");
    let (code, message) = envelope
        .error
        .map(|e| (e.code, e.message))
        .unwrap_or_else(|| ("ERROR".to_owned(), err.to_string()));
    let mut event = json!({
        "type": "error",
        "ok": false,
        "error": { "code": code, "message": message },
        "next_actions": [],
    });
    if let Some(fix) = envelope.fix.filter(|f| !f.is_empty()) {
        event["fix"] = json!(fix);
    }
    event
}

/// Emits the terminal error event, then returns the error so the handler can
/// propagate it via `?` or `return Err(...)`.
async fn fail_check(
    sender: &StreamSender,
    err: cli_engine::CliCoreError,
) -> cli_engine::CliCoreError {
    sender.send(check_error_event(&err)).await;
    err
}

/// Points at `email create` (prefilling `--account-id` and any required
/// `--consent` flags) only when the response names an eligible account —
/// with no eligible account there is nothing to create. Always includes a
/// pointer at the `email` guide for what an account ID actually is.
fn eligibility_next_actions(email: &str, data: &types::EligibilityResult) -> Vec<NextAction> {
    let mut actions = Vec::new();

    if let Some(account) = data.eligible_accounts.first() {
        let mut command = "email create --email <email> --account-id <account-id>".to_owned();
        for requirement in &account.requirements {
            command.push_str(&format!(" --consent {}", requirement.type_));
        }
        actions.push(
            next_action(command, "Create a mailbox for this address")
                .with_param("email", NextActionParam::value(email.to_owned()))
                .with_param(
                    "account-id",
                    NextActionParam::value(account.account_id.to_string()),
                ),
        );
    }

    actions.push(next_action("guide email", "Learn about email accounts"));
    actions
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eligible_account(requirements: Vec<types::AgreementRequirement>) -> types::EligibleAccount {
        types::EligibleAccount {
            account_id: types::Uuid("acct-1".to_owned()),
            account_name: None,
            default: false,
            mailbox_type: types::MailboxType("TITAN".to_owned()),
            requirements,
        }
    }

    #[test]
    fn is_a_read_tier_command_scoped_to_email_read() {
        let spec = command().spec;
        assert_eq!(spec.tier, Some(Tier::Read));
        assert_eq!(spec.metadata().scopes, vec![EMAIL_READ.to_string()]);
    }

    #[test]
    fn next_actions_prefill_email_and_account_id_when_present() {
        let data = types::EligibilityResult {
            is_eligible: true,
            eligible_accounts: vec![eligible_account(vec![])],
        };
        let actions = eligibility_next_actions("someone@example.com", &data);
        assert_eq!(actions.len(), 2);
        assert_eq!(
            actions[0].command,
            "gddy email create --email <email> --account-id <account-id>"
        );
        assert_eq!(actions[1].command, "gddy guide email");
    }

    #[test]
    fn next_actions_omit_create_when_no_eligible_accounts() {
        let data = types::EligibilityResult {
            is_eligible: false,
            eligible_accounts: vec![],
        };
        let actions = eligibility_next_actions("someone@example.com", &data);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].command, "gddy guide email");
    }

    #[test]
    fn needs_titan_plan_fix_points_at_titan_catalog_search() {
        use cli_engine::build_error_envelope;

        let body = r#"{"message":"no plan","details":[{"issue":"NEEDS_TITAN_PLAN"}]}"#;
        let err = client_err_with_fix(
            ClientError::Http {
                status: 422,
                body: body.to_owned(),
            },
            "Run: 'gddy shopping catalog search --query titan' \
             to see available email plans, select one to purchase, then retry.",
        );
        let envelope = build_error_envelope(&err, "email");
        assert!(
            envelope
                .fix
                .as_deref()
                .is_some_and(|f| f.contains("--query titan")),
            "{envelope:?}"
        );
    }

    #[test]
    fn needs_microsoft_plan_fix_points_at_microsoft_catalog_search() {
        use cli_engine::build_error_envelope;

        let body = r#"{"message":"no plan","details":[{"issue":"NEEDS_MICROSOFT_PLAN"}]}"#;
        let err = client_err_with_fix(
            ClientError::Http {
                status: 422,
                body: body.to_owned(),
            },
            "Run: 'gddy shopping catalog search --query microsoft' \
             to see available Microsoft 365 email plans, select one to purchase, then retry.",
        );
        let envelope = build_error_envelope(&err, "email");
        assert!(
            envelope
                .fix
                .as_deref()
                .is_some_and(|f| f.contains("--query microsoft")),
            "{envelope:?}"
        );
    }

    #[test]
    fn next_actions_include_consent_flags_for_outstanding_requirements() {
        let data = types::EligibilityResult {
            is_eligible: true,
            eligible_accounts: vec![eligible_account(vec![types::AgreementRequirement {
                reference: None,
                title: None,
                type_: "FREETRIAL_AUTORENEW".to_owned(),
            }])],
        };
        let actions = eligibility_next_actions("someone@example.com", &data);
        assert_eq!(actions.len(), 2);
        assert_eq!(
            actions[0].command,
            "gddy email create --email <email> --account-id <account-id> --consent FREETRIAL_AUTORENEW"
        );
        assert_eq!(actions[1].command, "gddy guide email");
    }
}
