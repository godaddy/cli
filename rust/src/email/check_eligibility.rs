use cli_engine::{
    CommandSpec, NextAction, NextActionParam, RuntimeCommandSpec, StreamSender, Tier,
};
use email_client::types;
use serde_json::{Value, json};

use crate::email::client::ClientError;
use crate::email::common::fail_streaming;
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
                Err(e) => return Err(fail_streaming(&sender, e).await),
            };

            let data = match client.check_eligibility(&args.email).await {
                Ok(d) => d,
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
                    return Err(fail_streaming(
                        &sender,
                        client_err_with_fix(ClientError::Http { status: 422, body }, fix),
                    )
                    .await);
                }
                Err(e) => return Err(fail_streaming(&sender, client_err(e)).await),
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

/// Points at `email create` (prefilling `--account-id` and any required
/// `--consent` flags) only when the response names an eligible account —
/// with no eligible account there is nothing to create. Always includes a
/// pointer at the `email` guide for what an account ID actually is.
fn eligibility_next_actions(email: &str, data: &types::EligibilityResult) -> Vec<NextAction> {
    let mut actions = Vec::new();

    if let Some(account) = data.eligible_accounts.first() {
        let mut command = "email create --email <email> --account-id <account-id>".to_owned();
        for requirement in &account.requirements {
            if requirement.type_ != "MICROSOFT_MCA" {
                command.push_str(&format!(" --consent {}", requirement.type_));
            }
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

// Used to silence an unused-import warning when `Value` is only referenced in
// the error-event helper that was moved to `common`.
const _: fn() = || {
    let _: Value;
};

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
    fn next_actions_include_consent_flags_for_non_mca_requirements() {
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
    }

    #[test]
    fn next_actions_omit_consent_flag_for_mca_requirement() {
        let data = types::EligibilityResult {
            is_eligible: true,
            eligible_accounts: vec![eligible_account(vec![types::AgreementRequirement {
                reference: Some(
                    "https://cdn.partner.microsoft.com/mca/?attestationid=abc".to_owned(),
                ),
                title: Some("Microsoft Customer Agreement".to_owned()),
                type_: "MICROSOFT_MCA".to_owned(),
            }])],
        };
        let actions = eligibility_next_actions("someone@example.com", &data);
        assert_eq!(
            actions[0].command, "gddy email create --email <email> --account-id <account-id>",
            "MCA must not appear as a --consent flag — it is handled via browser attestation"
        );
    }
}
