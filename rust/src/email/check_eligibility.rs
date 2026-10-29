use cli_engine::{
    CommandSpec, NextAction, NextActionParam, RuntimeCommandSpec, StreamSender, Tier,
};
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
) -> cli_engine::Result<(Value, Vec<String>)> {
    match client.check_eligibility(email).await {
        Ok(data) => {
            // Requirements are nested inside each eligible account, not at
            // the top level; search across accounts for the first MCA gate.
            let mca_req = data
                .get("eligibleAccounts")
                .and_then(Value::as_array)
                .and_then(|accounts| {
                    accounts
                        .iter()
                        .find_map(terms_acceptance::extract_mca_requirement)
                });
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
            if body_has_issue(&body, "EMAIL_PLAN_NOT_AVAILABLE") =>
        {
            Err(client_err_with_fix(
                ClientError::Http { status: 422, body },
                "Run: 'gddy shopping catalog search --query titan' \
                 to see available email plans, select one to purchase, then retry.",
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
fn eligibility_next_actions(email: &str, data: &Value) -> Vec<NextAction> {
    let mut actions = Vec::new();

    if let Some(account) = first_eligible_account(data)
        && let Some(account_id) = account.get("accountId").and_then(|v| {
            v.as_str()
                .map(String::from)
                .or_else(|| v.as_i64().map(|n| n.to_string()))
        })
    {
        let mut command = "email create --email <email> --account-id <account-id>".to_owned();
        for requirement_type in requirement_types(account) {
            command.push_str(&format!(" --consent {requirement_type}"));
        }
        actions.push(
            next_action(command, "Create a mailbox for this address")
                .with_param("email", NextActionParam::value(email.to_owned()))
                .with_param("account-id", NextActionParam::value(account_id.to_owned())),
        );
    }

    actions.push(next_action("guide email", "Learn about email accounts"));
    actions
}

// Every field here is read out of an untyped `serde_json::Value`, as is every
// other `EmailClient` response in this module — panel-v3-api has no published
// OpenAPI spec yet. Once it does, a generated typed client (mirroring
// `domains_client`) would remove this class of bug; not actionable today.
fn first_eligible_account(data: &Value) -> Option<&Value> {
    data.get("eligibleAccounts")?.as_array()?.first()
}

fn requirement_types(account: &Value) -> Vec<&str> {
    account
        .get("requirements")
        .and_then(Value::as_array)
        .map(|reqs| {
            reqs.iter()
                .filter_map(|r| r.get("type").and_then(Value::as_str))
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn is_a_read_tier_command_scoped_to_email_read() {
        let spec = command().spec;
        assert_eq!(spec.tier, Some(Tier::Read));
        assert_eq!(spec.metadata().scopes, vec![EMAIL_READ.to_string()]);
    }

    #[test]
    fn next_actions_prefill_email_and_account_id_when_present() {
        let data = json!({
            "isEligible": true,
            "eligibleAccounts": [{ "accountId": "acct-1", "requirements": [] }]
        });
        let actions = eligibility_next_actions("someone@example.com", &data);
        assert_eq!(actions.len(), 2);
        assert_eq!(
            actions[0].command,
            "gddy email create --email <email> --account-id <account-id>"
        );
        assert_eq!(actions[1].command, "gddy guide email");
    }

    #[test]
    fn next_actions_prefill_numeric_account_id() {
        let data = json!({
            "isEligible": true,
            "eligibleAccounts": [{ "accountId": 1401049, "requirements": [] }]
        });
        let actions = eligibility_next_actions("someone@example.com", &data);
        assert_eq!(actions.len(), 2);
        assert_eq!(
            actions[0].command,
            "gddy email create --email <email> --account-id <account-id>"
        );
        let params = &actions[0].params;
        assert_eq!(
            params.get("account-id").and_then(|p| p.value.as_deref()),
            Some("1401049")
        );
    }

    #[test]
    fn next_actions_omit_create_when_no_eligible_accounts() {
        let data = json!({
            "isEligible": false,
            "ineligibilityReasons": [
                { "type": "NO_ELIGIBLE_ACCOUNT", "message": "No eligible account was found." }
            ]
        });
        let actions = eligibility_next_actions("someone@example.com", &data);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].command, "gddy guide email");
    }

    #[test]
    fn email_plan_not_available_fix_points_at_shopping_catalog() {
        use cli_engine::build_error_envelope;

        let body = r#"{"message":"no plan","details":[{"issue":"EMAIL_PLAN_NOT_AVAILABLE"}]}"#;
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
                .is_some_and(|f| f.contains("shopping catalog search")),
            "{envelope:?}"
        );
    }

    #[test]
    fn next_actions_include_consent_flags_for_outstanding_requirements() {
        let data = json!({
            "isEligible": true,
            "eligibleAccounts": [{
                "accountId": "acct-1",
                "requirements": [{ "type": "FREETRIAL_AUTORENEW" }]
            }]
        });
        let actions = eligibility_next_actions("someone@example.com", &data);
        assert_eq!(actions.len(), 2);
        assert_eq!(
            actions[0].command,
            "gddy email create --email <email> --account-id <account-id> --consent FREETRIAL_AUTORENEW"
        );
        assert_eq!(actions[1].command, "gddy guide email");
    }
}
