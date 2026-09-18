use cli_engine::{
    CliCoreError, CommandResult, CommandSpec, NextAction, NextActionParam, RuntimeCommandSpec, Tier,
};
use email_client::types;

use crate::email::{client_err, make_client};
use crate::next_action::next_action;
use crate::scopes::EMAIL_READ;

#[derive(Debug, Clone, clap::Args)]
struct CheckEligibilityArgs {
    #[arg(long, value_name = "EMAIL")]
    email: String,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<CheckEligibilityArgs, _, _, _>(
        CommandSpec::from_args::<CheckEligibilityArgs>(
            "check-eligibility",
            "Check whether an email address is eligible for a new mailbox",
        )
        .with_system("email")
        .with_tier(Tier::Read)
        .with_json_schema::<types::EligibilityResult>()
        .with_scopes(&[EMAIL_READ]),
        |ctx, args: CheckEligibilityArgs| async move {
            let client = make_client(&ctx, &[EMAIL_READ]).await?;
            let data = client
                .check_eligibility(&args.email)
                .await
                .map_err(client_err)?;
            let next_actions = eligibility_next_actions(&args.email, &data);
            let value = serde_json::to_value(&data).map_err(|e| {
                CliCoreError::message(format!("failed to serialize eligibility result: {e}"))
            })?;
            Ok(CommandResult::new(value).with_next_actions(next_actions))
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

    #[test]
    fn is_a_read_tier_command_scoped_to_email_read() {
        let spec = command().spec;
        assert_eq!(spec.tier, Some(Tier::Read));
        assert_eq!(spec.metadata().scopes, vec![EMAIL_READ.to_string()]);
    }

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
