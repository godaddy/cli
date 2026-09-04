use cli_engine::{
    CommandResult, CommandSpec, NextAction, NextActionParam, RuntimeCommandSpec, Tier,
};
use serde_json::Value;

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
        .with_scopes(&[EMAIL_READ]),
        |ctx, args: CheckEligibilityArgs| async move {
            let client = make_client(&ctx, &[EMAIL_READ]).await?;
            let data = client
                .check_eligibility(&args.email)
                .await
                .map_err(client_err)?;
            let next_actions = eligibility_next_actions(&args.email, &data);
            Ok(CommandResult::new(data).with_next_actions(next_actions))
        },
    )
}

/// Points at `email create` (prefilling `--account-id` and any required
/// `--consent` flags) only when the response names an eligible account —
/// with no eligible account there is nothing to create. Always includes a
/// pointer at the `email` guide for what an account ID actually is.
fn eligibility_next_actions(email: &str, data: &Value) -> Vec<NextAction> {
    let mut actions = Vec::new();

    if let Some(account) = first_eligible_account(data)
        && let Some(account_id) = account.get("accountId").and_then(Value::as_str)
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
