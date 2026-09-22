use cli_engine::{
    CommandSpec, NextAction, NextActionParam, RuntimeCommandSpec, StreamSender, Tier,
};
use serde_json::{Value, json};

use crate::email::check_eligibility;
use crate::email::client::ClientError;
use crate::email::terms_acceptance;
use crate::email::{body_has_issue, client_err, client_err_with_fix, make_client};
use crate::next_action::next_action;
use crate::scopes::{EMAIL_CREATE, EMAIL_READ};

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

fn request_body(args: &CreateArgs, resolved_account_id: &str, extra_consents: &[String]) -> Value {
    let mut body = serde_json::Map::new();
    body.insert("emailAddress".to_owned(), json!(args.email));
    body.insert("accountId".to_owned(), json!(resolved_account_id));
    if let Some(first_name) = &args.first_name {
        body.insert("firstName".to_owned(), json!(first_name));
    }
    if let Some(last_name) = &args.last_name {
        body.insert("lastName".to_owned(), json!(last_name));
    }
    // Merge explicit --consent args with types accepted interactively (e.g. MCA).
    let mut all_consents: Vec<&str> = args.consent.iter().map(String::as_str).collect();
    for t in extra_consents {
        if !all_consents.contains(&t.as_str()) {
            all_consents.push(t.as_str());
        }
    }
    if !all_consents.is_empty() {
        let consents: Vec<Value> = all_consents.iter().map(|t| json!({ "type": t })).collect();
        body.insert("consents".to_owned(), json!(consents));
    }
    Value::Object(body)
}

/// Resolves the account ID to use for mailbox creation.
/// Uses the caller-supplied `--account-id` when present; otherwise picks the
/// default eligible account (or the first one) from the eligibility response.
fn resolve_account_id(arg: &Option<String>, eligibility: &Value) -> cli_engine::Result<String> {
    if let Some(id) = arg {
        return Ok(id.clone());
    }
    eligibility
        .get("eligibleAccounts")
        .and_then(Value::as_array)
        .and_then(|accounts| {
            accounts
                .iter()
                .find(|a| a.get("default").and_then(Value::as_bool).unwrap_or(false))
                .or_else(|| accounts.first())
        })
        .and_then(|account| {
            account.get("accountId").and_then(|v| {
                v.as_str()
                    .map(String::from)
                    .or_else(|| v.as_i64().map(|n| n.to_string()))
            })
        })
        .ok_or_else(|| {
            crate::error::GddyError::validation("No eligible account found for this email address.")
                .with_fix("Run: gddy email check-eligibility --email <email>")
                .into_cli_error()
        })
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
    RuntimeCommandSpec::new_typed_streaming::<CreateArgs, _, _>(
        CommandSpec::from_args::<CreateArgs>("create", "Create a new Email mailbox")
            .with_system("email")
            .with_tier(Tier::Mutate)
            .mutates(true)
            .with_scopes(&[EMAIL_CREATE, EMAIL_READ]),
        |ctx, args: CreateArgs, sender: StreamSender| async move {
            let client = match make_client(&ctx, &[EMAIL_CREATE, EMAIL_READ]).await {
                Ok(c) => c,
                Err(e) => return Err(fail_create(&sender, e).await),
            };

            // Step 1: Check eligibility; auto-accept MCA via browser if required.
            let (eligibility, accepted_types) =
                match check_eligibility::run_eligibility_with_mca(&client, &args.email, &sender)
                    .await
                {
                    Ok(result) => result,
                    Err(e) => return Err(fail_create(&sender, e).await),
                };

            // Step 2: Resolve account ID from --account-id or the default eligible account.
            let account_id = match resolve_account_id(&args.account_id, &eligibility) {
                Ok(id) => id,
                Err(e) => return Err(fail_create(&sender, e).await),
            };

            // Step 3: Create the mailbox. If the API requires fresh MCA consent
            // (CONSENT_NOT_PROVIDED with an embedded attestation URL), open the
            // browser, wait for acceptance, then retry once with the consent.
            let mut extra_consents = accepted_types;
            let data = loop {
                let body = request_body(&args, &account_id, &extra_consents);
                match client.create_mailbox(body).await {
                    Ok(d) => break d,

                    Err(ClientError::Http { status: 422, body: err_body })
                        if body_has_issue(&err_body, "CONSENT_NOT_PROVIDED")
                            && !extra_consents.iter().any(|c| c == "MICROSOFT_MCA") =>
                    {
                        match mca_requirement_from_consent_error(&err_body) {
                            Some(req) => {
                                if let Err(e) =
                                    terms_acceptance::wait_for_acceptance(&req, &sender).await
                                {
                                    return Err(fail_create(&sender, e).await);
                                }
                                extra_consents.push("MICROSOFT_MCA".to_owned());
                                // loop to retry create with consent
                            }
                            None => {
                                return Err(fail_create(
                                    &sender,
                                    client_err_with_fix(
                                        ClientError::Http {
                                            status: 422,
                                            body: err_body,
                                        },
                                        format!(
                                            "This looks like a business-rule failure (missing \
                                             agreements or no eligible account). Run: gddy email \
                                             check-eligibility --email {}",
                                            args.email
                                        ),
                                    ),
                                )
                                .await);
                            }
                        }
                    }

                    Err(ClientError::Http { status: 422, body })
                        if body_has_issue(&body, "EMAIL_PLAN_NOT_AVAILABLE") =>
                    {
                        return Err(fail_create(
                            &sender,
                            client_err_with_fix(
                                ClientError::Http { status: 422, body },
                                "Run: 'gddy shopping catalog search --query titan' \
                                 to see available email plans, select one to purchase, then retry.",
                            ),
                        )
                        .await);
                    }

                    Err(ClientError::Http { status, body })
                        if status == 422 && body_has_issue(&body, "EMAIL_PLAN_NOT_ELIGIBLE") =>
                    {
                        return Err(fail_create(
                            &sender,
                            client_err_with_fix(
                                ClientError::Http { status, body },
                                "The selected account is not eligible to provision this mailbox \
                                 via API. Try a different account ID from \
                                 'gddy email check-eligibility', or provision through the \
                                 GoDaddy control panel.",
                            ),
                        )
                        .await);
                    }

                    Err(ClientError::Http { status, body })
                        if status == 400 || status == 422 =>
                    {
                        return Err(fail_create(
                            &sender,
                            client_err_with_fix(
                                ClientError::Http { status, body },
                                format!(
                                    "This looks like a business-rule failure (missing agreements \
                                     or no eligible account). Run: gddy email \
                                     check-eligibility --email {}",
                                    args.email
                                ),
                            ),
                        )
                        .await);
                    }

                    Err(e) => return Err(fail_create(&sender, client_err(e)).await),
                }
            };

            let next_actions = create_next_actions(&data);
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

/// Extracts the MCA [`terms_acceptance::TermsRequirement`] from a 422
/// `CONSENT_NOT_PROVIDED` error body. The create API embeds the attestation
/// URL in the `details[].description` field of those errors, e.g.:
///
/// ```json
/// {"details": [{"issue": "CONSENT_NOT_PROVIDED",
///               "description": "...at https://cdn.partner.microsoft.com/mca/?attestationid=..."}]}
/// ```
fn mca_requirement_from_consent_error(
    body: &str,
) -> Option<terms_acceptance::TermsRequirement> {
    #[derive(serde::Deserialize)]
    struct Detail {
        issue: Option<String>,
        description: Option<String>,
    }
    #[derive(serde::Deserialize)]
    struct Body {
        #[serde(default)]
        details: Vec<Detail>,
    }
    let parsed: Body = serde_json::from_str(body).ok()?;
    let description = parsed
        .details
        .iter()
        .find(|d| d.issue.as_deref() == Some("CONSENT_NOT_PROVIDED"))?
        .description
        .as_deref()?;
    // The description embeds the URL:
    // "...at https://cdn.partner.microsoft.com/mca/?attestationid=<id> before..."
    let url = description
        .find("https://")
        .map(|i| &description[i..])
        .and_then(|s| s.split_ascii_whitespace().next())
        .map(str::to_owned)?;
    terms_acceptance::TermsRequirement::from_url(url)
}

fn create_error_event(err: &cli_engine::CliCoreError) -> Value {
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

async fn fail_create(
    sender: &StreamSender,
    err: cli_engine::CliCoreError,
) -> cli_engine::CliCoreError {
    sender.send(create_error_event(&err)).await;
    err
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_a_mutate_tier_command_scoped_to_email_create_and_read() {
        let spec = command().spec;
        assert_eq!(spec.tier, Some(Tier::Mutate));
        assert!(spec.mutates);
        let scopes = spec.metadata().scopes;
        assert!(scopes.contains(&EMAIL_CREATE.to_string()));
        assert!(scopes.contains(&EMAIL_READ.to_string()));
    }

    #[test]
    fn request_body_includes_resolved_account_id_and_merges_consents() {
        let args = CreateArgs {
            email: "someone@example.com".to_owned(),
            account_id: None,
            first_name: None,
            last_name: None,
            consent: vec!["FREETRIAL_AUTORENEW".to_owned()],
        };
        let body = request_body(&args, "1401049", &["MICROSOFT_MCA".to_owned()]);
        assert_eq!(body["emailAddress"], "someone@example.com");
        assert_eq!(body["accountId"], "1401049");
        assert!(body.get("firstName").is_none());
        let consents = body["consents"].as_array().expect("consents array");
        assert_eq!(consents.len(), 2);
        assert!(consents.iter().any(|c| c["type"] == "FREETRIAL_AUTORENEW"));
        assert!(consents.iter().any(|c| c["type"] == "MICROSOFT_MCA"));
    }

    #[test]
    fn request_body_deduplicates_consents() {
        let args = CreateArgs {
            email: "someone@example.com".to_owned(),
            account_id: None,
            first_name: None,
            last_name: None,
            consent: vec!["MICROSOFT_MCA".to_owned()],
        };
        let body = request_body(&args, "1401049", &["MICROSOFT_MCA".to_owned()]);
        let consents = body["consents"].as_array().expect("consents array");
        assert_eq!(consents.len(), 1);
    }

    #[test]
    fn resolve_account_id_prefers_explicit_arg() {
        let eligibility = json!({
            "eligibleAccounts": [{ "accountId": 1401049, "default": true }]
        });
        let id = resolve_account_id(&Some("explicit-123".to_owned()), &eligibility)
            .expect("should resolve");
        assert_eq!(id, "explicit-123");
    }

    #[test]
    fn resolve_account_id_picks_default_account_when_no_arg() {
        let eligibility = json!({
            "eligibleAccounts": [
                { "accountId": 1401049, "default": false },
                { "accountId": 1401050, "default": true }
            ]
        });
        let id = resolve_account_id(&None, &eligibility).expect("should resolve");
        assert_eq!(id, "1401050");
    }

    #[test]
    fn resolve_account_id_falls_back_to_first_when_no_default() {
        let eligibility = json!({
            "eligibleAccounts": [
                { "accountId": 1401049, "default": false },
                { "accountId": 1401050, "default": false }
            ]
        });
        let id = resolve_account_id(&None, &eligibility).expect("should resolve");
        assert_eq!(id, "1401049");
    }

    #[test]
    fn resolve_account_id_errors_when_no_eligible_accounts() {
        let eligibility = json!({ "isEligible": false, "eligibleAccounts": [] });
        assert!(resolve_account_id(&None, &eligibility).is_err());
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
}
