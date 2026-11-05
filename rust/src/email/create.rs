use std::io::IsTerminal as _;

use cli_engine::{
    CliCoreError, CommandSpec, NextAction, NextActionParam, RuntimeCommandSpec, StreamSender, Tier,
};
use email_client::types;
use serde_json::{Value, json};

use crate::email::client::ClientError;
use crate::email::{body_has_issue, client_err, client_err_with_fix, make_client};
use crate::error::GddyError;
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
    /// Not needed for `MICROSOFT_MCA` — that is handled automatically.
    #[arg(long, value_name = "REQUIREMENT_TYPE")]
    consent: Vec<String>,
}

fn request_body(args: &CreateArgs) -> types::CreateMailboxBody {
    types::CreateMailboxBody {
        account_id: args.account_id.clone().map(types::Uuid),
        consents: args
            .consent
            .iter()
            .map(|type_| types::ConsentRequest {
                type_: type_.clone(),
            })
            .collect(),
        created_at: None,
        display_name: None,
        email_address: args.email.clone(),
        first_name: args.first_name.clone(),
        last_name: args.last_name.clone(),
        links: vec![],
        mailbox_id: None,
        mailbox_type: None,
        status: None,
        updated_at: None,
    }
}

fn create_next_actions(data: &types::CreateMailboxResponse) -> Vec<NextAction> {
    let Some(mailbox_id) = &data.mailbox_id else {
        return Vec::new();
    };
    vec![
        next_action(
            "email get <mailbox-id>",
            "Poll the mailbox until its status reaches COMPLETED",
        )
        .with_param("mailbox-id", NextActionParam::value(mailbox_id.to_string())),
    ]
}

fn create_error_event(err: &CliCoreError) -> Value {
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

async fn fail_create(sender: &StreamSender, err: CliCoreError) -> CliCoreError {
    sender.send(create_error_event(&err)).await;
    err
}

async fn tap_create_err<T>(
    sender: &StreamSender,
    result: cli_engine::Result<T>,
) -> cli_engine::Result<T> {
    match result {
        Ok(v) => Ok(v),
        Err(e) => Err(fail_create(sender, e).await),
    }
}

fn create_result_event(data: &types::CreateMailboxResponse) -> cli_engine::Result<Value> {
    let next_actions = create_next_actions(data);
    let result = serde_json::to_value(data)
        .map_err(|e| CliCoreError::message(format!("failed to serialize create response: {e}")))?;
    Ok(json!({
        "type": "result",
        "ok": true,
        "result": result,
        "next_actions": next_actions,
    }))
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_streaming::<CreateArgs, _, _>(
        CommandSpec::from_args::<CreateArgs>("create", "Create a new Email mailbox")
            .with_long(
                "Create a new Email mailbox for an address on a domain you own. \
                 Run `gddy email check-eligibility --email <address>` first to \
                 find the correct `--account-id` and any `--consent` requirements.\n\
                 \n\
                 AI assistants: If a Microsoft Customer Agreement must be accepted, \
                 this command opens a browser automatically and polls until acceptance \
                 is confirmed (up to 5 minutes). When running non-interactively, the \
                 command exits immediately with the agreement URL in the error's `fix` \
                 field — open that URL in a browser, accept the agreement, then retry.",
            )
            .with_system("email")
            .with_tier(Tier::Mutate)
            .mutates(true)
            .with_json_schema::<types::CreateMailboxResponse>()
            .with_scopes(&[EMAIL_CREATE]),
        |ctx, args: CreateArgs, sender: StreamSender| async move {
            let client = tap_create_err(&sender, make_client(&ctx, &[EMAIL_CREATE]).await).await?;

            // Check eligibility first to detect any MICROSOFT_MCA requirement.
            // If one is present, open the acceptance URL in the browser and wait
            // for acceptance before creating the mailbox. Eligibility errors are
            // ignored here — the create call will surface plan issues with clearer messages.
            if let Ok(eligibility) = client.check_eligibility(&args.email).await
                && let Some(req) = eligibility
                    .eligible_accounts
                    .iter()
                    .find_map(super::terms_acceptance::extract_mca_requirement)
            {
                if !std::io::stdin().is_terminal() {
                    let err = GddyError::validation(
                        "Microsoft Customer Agreement acceptance is required but \
                         cannot be completed non-interactively.",
                    )
                    .with_fix(format!(
                        "Open the following URL in a browser, accept the Microsoft \
                         Customer Agreement, then re-run this command: {}",
                        req.acceptance_url
                    ))
                    .into_cli_error();
                    return Err(fail_create(&sender, err).await);
                }

                let browser_opened = open::that(&req.acceptance_url).is_ok();
                if browser_opened {
                    sender
                        .send(json!({
                            "type": "step",
                            "name": "mca",
                            "status": "started",
                            "message": "A browser tab was opened to accept the Microsoft Customer Agreement. \
                                        Once you accept, mailbox creation will continue automatically — \
                                        no need to re-run the command.",
                        }))
                        .await;
                } else {
                    let err = GddyError::validation("Could not open a browser automatically.")
                        .with_fix(format!(
                            "Open this URL in a browser, accept the Microsoft Customer Agreement, \
                             then re-run this command: {}",
                            req.acceptance_url
                        ))
                        .into_cli_error();
                    return Err(fail_create(&sender, err).await);
                }

                tap_create_err(
                    &sender,
                    super::terms_acceptance::wait_for_mca_acceptance(&client, &args.email).await,
                )
                .await?;
            }

            let body = request_body(&args);
            let data = tap_create_err(
                &sender,
                client.create_mailbox(body).await.map_err(|e| match &e {
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
                    ClientError::Http { status, .. } if *status == 400 || *status == 422 => {
                        client_err_with_fix(
                            e,
                            format!(
                                "This looks like a business-rule failure (missing agreements \
                                 or no eligible account). Run: gddy email check-eligibility \
                                 --email {}",
                                args.email
                            ),
                        )
                    }
                    _ => client_err(e),
                }),
            )
            .await?;

            let event = tap_create_err(&sender, create_result_event(&data)).await?;
            sender.send(event).await;
            Ok(())
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_a_streaming_mutate_tier_command_scoped_to_email_create() {
        let spec = command().spec;
        assert_eq!(spec.tier, Some(Tier::Mutate));
        assert!(spec.mutates);
        assert_eq!(spec.metadata().scopes, vec![EMAIL_CREATE.to_string()]);
    }

    #[test]
    fn help_text_includes_ai_assistants_note() {
        let spec = command().spec;
        let long = spec.long.as_deref().unwrap_or("");
        assert!(
            long.contains("AI assistants:"),
            "long help must include an AI assistants note; got: {long:?}"
        );
        assert!(
            long.contains("non-interactively"),
            "AI note must mention non-interactive behavior; got: {long:?}"
        );
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
        assert_eq!(body.email_address, "someone@example.com");
        assert_eq!(
            body.account_id.map(|id| id.to_string()),
            Some("acct-1".to_owned())
        );
        assert!(body.first_name.is_none());
        assert_eq!(
            body.consents
                .into_iter()
                .map(|c| c.type_)
                .collect::<Vec<_>>(),
            vec!["EMAIL_TOS".to_owned()]
        );
    }

    #[test]
    fn create_next_actions_points_at_get_when_mailbox_id_present() {
        let data = types::CreateMailboxResponse {
            mailbox_id: Some(types::Uuid("mb-1".to_owned())),
            status: Some(types::MailboxStatus("EXECUTING".to_owned())),
            ..Default::default()
        };
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
        let data = types::CreateMailboxResponse {
            status: Some(types::MailboxStatus("EXECUTING".to_owned())),
            ..Default::default()
        };
        assert!(create_next_actions(&data).is_empty());
    }

    #[test]
    fn create_result_event_has_correct_shape() {
        let data = types::CreateMailboxResponse {
            mailbox_id: Some(types::Uuid("mb-1".to_owned())),
            status: Some(types::MailboxStatus("EXECUTING".to_owned())),
            ..Default::default()
        };
        let event = create_result_event(&data).expect("serialization must succeed");
        assert_eq!(event["type"], "result");
        assert_eq!(event["ok"], true);
        assert_eq!(event["result"]["mailboxId"], "mb-1");
        assert_eq!(event["result"]["status"], "EXECUTING");
        assert!(event["next_actions"].is_array());
    }

    #[test]
    fn create_error_event_has_correct_shape() {
        let err = GddyError::validation("something went wrong")
            .with_fix("run check-eligibility")
            .into_cli_error();
        let event = create_error_event(&err);
        assert_eq!(event["type"], "error");
        assert_eq!(event["ok"], false);
        assert!(event["error"]["message"].is_string());
        assert!(
            event["fix"]
                .as_str()
                .is_some_and(|f| f.contains("check-eligibility")),
            "fix must be present in error event: {event:?}"
        );
    }

    #[test]
    fn non_interactive_error_contains_acceptance_url_in_fix() {
        let url = "https://cdn.partner.microsoft.com/mca/?attestationid=abc-123";
        let err = GddyError::validation(
            "Microsoft Customer Agreement acceptance is required but \
             cannot be completed non-interactively.",
        )
        .with_fix(format!(
            "Open the following URL in a browser, accept the Microsoft \
             Customer Agreement, then re-run this command: {url}",
        ))
        .into_cli_error();
        let event = create_error_event(&err);
        assert_eq!(event["type"], "error");
        assert!(
            event["fix"].as_str().is_some_and(|f| f.contains(url)),
            "fix must contain the acceptance URL; got: {:?}",
            event["fix"]
        );
    }

    #[test]
    fn browser_launch_failure_error_contains_acceptance_url_in_fix() {
        let url = "https://cdn.partner.microsoft.com/mca/?attestationid=def-456";
        let err = GddyError::validation("Could not open a browser automatically.")
            .with_fix(format!(
                "Open this URL in a browser, accept the Microsoft Customer Agreement, \
                 then re-run this command: {url}",
            ))
            .into_cli_error();
        let event = create_error_event(&err);
        assert!(
            event["fix"].as_str().is_some_and(|f| f.contains(url)),
            "fix must contain the acceptance URL; got: {:?}",
            event["fix"]
        );
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
