use cli_engine::{
    CommandResult, CommandSpec, NextAction, NextActionParam, RuntimeCommandSpec, Tier,
};
use serde_json::{Value, json};

use crate::email::client::ClientError;
use crate::email::{client_err, client_err_with_fix, make_client};
use crate::next_action::next_action;
use crate::scopes::EMAIL_CREATE;

#[derive(Debug, Clone, clap::Args)]
struct CreateArgs {
    /// Email address for the new mailbox.
    #[arg(long, value_name = "EMAIL")]
    email: String,
    /// ID of an existing eligible account to provision this mailbox under,
    /// from `check-eligibility`'s `eligibleAccounts[].accountId` (see
    /// `gddy guide email`). Not a shopper/customer ID.
    #[arg(long = "account-id", value_name = "ACCOUNT_ID")]
    account_id: Option<String>,
    /// First name of the mailbox owner.
    #[arg(long = "first-name", value_name = "FIRST_NAME")]
    first_name: Option<String>,
    /// Last name of the mailbox owner.
    #[arg(long = "last-name", value_name = "LAST_NAME")]
    last_name: Option<String>,
    /// Requirement types the caller has obtained consent for (from
    /// `check-eligibility`'s `eligibleAccounts[].requirements[].type`), e.g.
    /// `FREETRIAL_AUTORENEW`. Repeatable: `--consent FREETRIAL_AUTORENEW`.
    #[arg(long, value_name = "REQUIREMENT_TYPE")]
    consent: Vec<String>,
}

fn request_body(args: &CreateArgs) -> Value {
    let mut body = serde_json::Map::new();
    body.insert("emailAddress".to_owned(), json!(args.email));
    if let Some(account_id) = &args.account_id {
        body.insert("accountId".to_owned(), json!(account_id));
    }
    if let Some(first_name) = &args.first_name {
        body.insert("firstName".to_owned(), json!(first_name));
    }
    if let Some(last_name) = &args.last_name {
        body.insert("lastName".to_owned(), json!(last_name));
    }
    if !args.consent.is_empty() {
        let consents: Vec<Value> = args.consent.iter().map(|t| json!({ "type": t })).collect();
        body.insert("consents".to_owned(), json!(consents));
    }
    Value::Object(body)
}

fn create_next_actions(data: &Value) -> Vec<NextAction> {
    let Some(mailbox_id) = data.get("mailboxId").and_then(Value::as_str) else {
        return Vec::new();
    };
    vec![
        next_action(
            "email get <mailbox-id>",
            "Poll the mailbox until its status reaches COMPLETED",
        )
        .with_param("mailbox-id", NextActionParam::value(mailbox_id.to_owned())),
    ]
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<CreateArgs, _, _, _>(
        CommandSpec::from_args::<CreateArgs>("create", "Create a new Email mailbox")
            .with_system("email")
            .with_tier(Tier::Mutate)
            .mutates(true)
            .with_scopes(&[EMAIL_CREATE]),
        |ctx, args: CreateArgs| async move {
            let client = make_client(&ctx, &[EMAIL_CREATE]).await?;
            let body = request_body(&args);
            let data = client.create_mailbox(body).await.map_err(|e| match &e {
                ClientError::Http { status, .. } if *status == 400 || *status == 422 => {
                    client_err_with_fix(
                        e,
                        format!(
                            "This looks like a business-rule failure (missing agreements or no \
                             eligible account). Run: gddy email check-eligibility --email {}",
                            args.email
                        ),
                    )
                }
                _ => client_err(e),
            })?;
            let next_actions = create_next_actions(&data);
            Ok(CommandResult::new(data).with_next_actions(next_actions))
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_a_mutate_tier_command_scoped_to_email_create() {
        let spec = command().spec;
        assert_eq!(spec.tier, Some(Tier::Mutate));
        assert!(spec.mutates);
        assert_eq!(spec.metadata().scopes, vec![EMAIL_CREATE.to_string()]);
    }

    #[test]
    fn request_body_includes_optional_fields_only_when_present() {
        let args = CreateArgs {
            email: "someone@example.com".to_owned(),
            account_id: Some("acct-1".to_owned()),
            first_name: None,
            last_name: None,
            consent: vec!["EMAIL_TOS".to_owned()],
        };
        let body = request_body(&args);
        assert_eq!(body["emailAddress"], "someone@example.com");
        assert_eq!(body["accountId"], "acct-1");
        assert!(body.get("firstName").is_none());
        assert_eq!(body["consents"], json!([{ "type": "EMAIL_TOS" }]));
    }

    #[test]
    fn create_next_actions_points_at_get_when_mailbox_id_present() {
        let data = json!({ "mailboxId": "mb-1", "status": "EXECUTING" });
        let actions = create_next_actions(&data);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].command, "gddy email get <mailbox-id>");
        assert_eq!(
            actions[0].params["mailbox-id"].value,
            Some("mb-1".to_owned())
        );
    }

    #[test]
    fn create_next_actions_empty_when_mailbox_id_absent() {
        let data = json!({ "status": "EXECUTING" });
        assert!(create_next_actions(&data).is_empty());
    }
}
