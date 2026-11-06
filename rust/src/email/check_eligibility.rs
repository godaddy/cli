use cli_engine::{
    CliCoreError, CommandResult, CommandSpec, NextAction, NextActionParam, RuntimeCommandSpec, Tier,
};
use email_client::types;

use crate::email::client::ClientError;
use crate::email::{body_has_issue, client_err, client_err_with_fix, make_client};
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
                .map_err(|e| match &e {
                    ClientError::Http { status, body }
                        if (*status == 422)
                            && (body_has_issue(body, "NEEDS_TITAN_PLAN")
                                || body_has_issue(body, "NEEDS_MICROSOFT_PLAN")
                                || body_has_issue(body, "EMAIL_PLAN_NOT_AVAILABLE")) =>
                    {
                        let fix = if body_has_issue(body, "NEEDS_MICROSOFT_PLAN") {
                            "Run: 'gddy shopping catalog search --query microsoft' \
                             to see available Microsoft 365 email plans, select one to purchase, then retry."
                        } else {
                            "Run: 'gddy shopping catalog search --query titan' \
                             to see available email plans, select one to purchase, then retry."
                        };
                        client_err_with_fix(e, fix)
                    }
                    ClientError::Http { status, body }
                        if *status == 422 && body_has_issue(body, "EMAIL_PLAN_NOT_ELIGIBLE") =>
                    {
                        client_err_with_fix(
                            e,
                            "Go to the GoDaddy Email dashboard \
                             (https://productivity.godaddy.com/addnewemail) to create the \
                             mailbox manually — this domain's plan doesn't support \
                             provisioning via this API.",
                        )
                    }
                    ClientError::Http { status, body }
                        if *status == 422
                            && body_has_issue(body, "EMAIL_ADDRESS_ALREADY_EXISTS") =>
                    {
                        client_err_with_fix(e, "Choose a different username.")
                    }
                    ClientError::Http { status, body }
                        if *status == 422
                            && body_has_issue(body, "USER_HAS_INVALID_DATA") =>
                    {
                        client_err_with_fix(
                            e,
                            "Your account information is incomplete or invalid. \
                             Please update your GoDaddy account details before creating \
                             an email address.",
                        )
                    }
                    ClientError::Http { status, body }
                        if *status == 422 && body_has_issue(body, "USER_HAS_NO_TAX_ID") =>
                    {
                        client_err_with_fix(
                            e,
                            "A Tax ID is required to create a Microsoft 365 email address. \
                             Please update your account information.",
                        )
                    }
                    _ => client_err(e),
                })?;
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
        let consents: Vec<&str> = account
            .requirements
            .iter()
            .filter(|r| r.type_ != "MICROSOFT_MCA")
            .map(|r| r.type_.as_str())
            .collect();

        let mut command = "email create --email <email> --account-id <account-id>".to_owned();
        for i in 0..consents.len() {
            command.push_str(&format!(" --consent <consent-{i}>"));
        }

        let mut action = next_action(command, "Create a mailbox for this address")
            .with_param("email", NextActionParam::value(email.to_owned()))
            .with_param(
                "account-id",
                NextActionParam::value(account.account_id.to_string()),
            );
        for (i, type_) in consents.iter().enumerate() {
            action = action.with_param(
                format!("consent-{i}"),
                NextActionParam::value((*type_).to_owned()),
            );
        }
        actions.push(action);
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
            "gddy email create --email <email> --account-id <account-id> --consent <consent-0>"
        );
        assert_eq!(
            actions[0].params["consent-0"].value.as_deref(),
            Some("FREETRIAL_AUTORENEW"),
            "consent value must be a structured param, not interpolated into the template"
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
        assert!(
            !actions[0].params.contains_key("consent-0"),
            "MCA must not be passed as a consent param"
        );
    }

    #[test]
    fn next_actions_include_multiple_consent_placeholders_for_multiple_requirements() {
        let data = types::EligibilityResult {
            is_eligible: true,
            eligible_accounts: vec![eligible_account(vec![
                types::AgreementRequirement {
                    reference: None,
                    title: None,
                    type_: "FREETRIAL_AUTORENEW".to_owned(),
                },
                types::AgreementRequirement {
                    reference: None,
                    title: None,
                    type_: "EMAIL_TOS".to_owned(),
                },
            ])],
        };
        let actions = eligibility_next_actions("someone@example.com", &data);
        assert_eq!(
            actions[0].command,
            "gddy email create --email <email> --account-id <account-id> --consent <consent-0> --consent <consent-1>"
        );
        assert_eq!(
            actions[0].params["consent-0"].value.as_deref(),
            Some("FREETRIAL_AUTORENEW")
        );
        assert_eq!(
            actions[0].params["consent-1"].value.as_deref(),
            Some("EMAIL_TOS")
        );
    }

    #[test]
    fn email_address_already_exists_fix_says_choose_different_username() {
        use cli_engine::build_error_envelope;

        let body =
            r#"{"message":"already exists","details":[{"issue":"EMAIL_ADDRESS_ALREADY_EXISTS"}]}"#;
        let err = client_err_with_fix(
            ClientError::Http {
                status: 422,
                body: body.to_owned(),
            },
            "Choose a different username.",
        );
        let envelope = build_error_envelope(&err, "email");
        assert!(
            envelope
                .fix
                .as_deref()
                .is_some_and(|f| f.contains("different username")),
            "{envelope:?}"
        );
    }

    #[test]
    fn email_plan_not_eligible_fix_points_at_the_dashboard() {
        use cli_engine::build_error_envelope;

        let body = r#"{"message":"not eligible","details":[{"issue":"EMAIL_PLAN_NOT_ELIGIBLE"}]}"#;
        let err = client_err_with_fix(
            ClientError::Http {
                status: 422,
                body: body.to_owned(),
            },
            "Go to the GoDaddy Email dashboard \
             (https://productivity.godaddy.com/addnewemail) to create the \
             mailbox manually — this domain's plan doesn't support \
             provisioning via this API.",
        );
        let envelope = build_error_envelope(&err, "email");
        assert!(
            envelope
                .fix
                .as_deref()
                .is_some_and(|f| f.contains("productivity.godaddy.com")),
            "{envelope:?}"
        );
    }
}
