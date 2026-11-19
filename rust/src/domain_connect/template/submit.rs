//! `gddy domain-connect template submit` — validate and submit a template.

use std::borrow::Cow;
use std::fmt;

use cli_engine::{
    CommandResult, CommandSpec, DetailedError, NextAction, NextActionParam, RuntimeCommandSpec,
    Tier,
};
use domains_client::types::{TemplateActionOutcome, TemplateActionResult};

use super::common::{
    SUBMIT_CONFIRMATION_NOTE, SYSTEM, read_template, result_json, submit, submit_columns, validate,
};
use crate::error::{GddyError, codes};
use crate::next_action::next_action;
use crate::output_schema::output_schema;
use crate::scopes::{DOMAIN_CONNECT_TEMPLATE_READ, DOMAIN_CONNECT_TEMPLATE_WRITE};

output_schema!(SubmitOutput {
    "file": "string";
    "providerId": "string";
    "serviceId": "string";
    "action": "string";
    "outcome": "string";
    "operation": "string", optional;
    "issues": "[]object";
    "pullRequest": "object", optional;
});

#[derive(Debug, Clone, clap::Args)]
struct Args {
    /// Template file written by `template init`, e.g. acme.com.email.json
    #[arg(value_name = "FILE")]
    file: String,
}

/// A template that was not submitted. The message lists every issue with its
/// code, field and fix; the next action re-runs the check, which returns the
/// same issues as structured data.
#[derive(Debug)]
struct Refused {
    file: String,
    message: String,
    blocked: bool,
}

impl Refused {
    fn new(file: &str, result: &TemplateActionResult) -> Self {
        let mut message = format!("{file} was not submitted: {}.", result.outcome);
        for issue in &result.issues {
            message.push_str(&format!(
                "\n  {} {}: {}",
                issue.level, issue.code, issue.message
            ));
            if let Some(field) = &issue.field {
                message.push_str(&format!("\n    field: {field}"));
            }
            if let Some(fix) = &issue.fix {
                message.push_str(&format!("\n    fix: {fix}"));
            }
        }
        Self {
            file: file.to_owned(),
            message,
            blocked: result.outcome == TemplateActionOutcome::Blocked,
        }
    }
}

impl fmt::Display for Refused {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for Refused {}

impl DetailedError for Refused {
    fn error_code(&self) -> Cow<'static, str> {
        Cow::Borrowed(codes::VALIDATION_ERROR)
    }

    fn error_system(&self) -> Option<Cow<'static, str>> {
        Some(Cow::Borrowed(SYSTEM))
    }

    fn error_request_id(&self) -> Option<Cow<'static, str>> {
        None
    }

    fn error_fix(&self) -> Option<Cow<'static, str>> {
        Some(Cow::Borrowed(if self.blocked {
            "Use a provider ID that is yours, or raise the template's version if you are \
             changing a service you already submitted. If the provider ID is yours and still \
             blocked, contact GoDaddy support."
        } else {
            "Fix the listed issues, then check the template again until the outcome is READY."
        }))
    }

    fn error_next_actions(&self) -> Vec<NextAction> {
        vec![
            next_action(
                "domain-connect template validate <file>",
                "Check the template again; this lists each issue with its field and fix",
            )
            .with_param("file", NextActionParam::value(self.file.as_str())),
        ]
    }
}

fn refusal(file: &str, result: &TemplateActionResult) -> cli_engine::CliCoreError {
    cli_engine::CliCoreError::with_detailed_error(Refused::new(file, result))
}

/// A result that neither submitted nor refused the template cannot be trusted
/// either way, so it is an error instead of a success with no pull request.
fn unexpected_outcome(file: &str, result: &TemplateActionResult) -> cli_engine::CliCoreError {
    GddyError::unexpected(format!(
        "{file}: the template service answered {} to a submit request.",
        result.outcome
    ))
    .with_fix("Run `gddy domain-connect template validate` to see where the template stands.")
    .with_system(SYSTEM)
    .into_cli_error()
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<Args, _, _, _>(
        CommandSpec::from_args::<Args>(
            "submit",
            "Submit a Domain Connect template for publication",
        )
        .with_long(format!(
            "Check a template and, when it is ready, submit it. GoDaddy stores the \
                 template, registers its provider ID to your account, and opens a pull \
                 request on the public Domain Connect templates repository (or updates the \
                 one you already have open). The output includes the pull request link, \
                 where you can follow the review.\n\
                 \n\
                 A template that is INVALID or BLOCKED is not submitted: the command lists \
                 the issues and exits with an error. Run `gddy domain-connect template \
                 validate` first to find them without submitting.\n\
                 \n\
                 Submitting is public and cannot be undone from the CLI. Pass --dry-run to \
                 run the same checks and see the result without submitting.\n\
                 \n\
                 {SUBMIT_CONFIRMATION_NOTE}\n\
                 \n\
                 Example:\n\
                   gddy domain-connect template submit acme.com.email.json"
        ))
        .with_system(SYSTEM)
        .with_tier(Tier::Mutate)
        .mutates(true)
        .handles_dry_run(true)
        .with_scopes(&[DOMAIN_CONNECT_TEMPLATE_READ, DOMAIN_CONNECT_TEMPLATE_WRITE])
        .with_output_schema::<SubmitOutput>()
        .with_view(submit_columns()),
        |ctx, args: Args| async move {
            let (path, template) = read_template(&args.file)?;
            let file = path.display().to_string();
            let client = crate::domain::common::make_client(&ctx).await?;

            if ctx.dry_run() {
                let result = validate(&client, &template).await?;
                if result.outcome != TemplateActionOutcome::Ready {
                    return Err(refusal(&file, &result));
                }
                return Ok(
                    CommandResult::new(result_json(&path, &template, &result)).with_dry_run()
                );
            }

            let result = submit(&client, &template).await?;
            match result.outcome {
                TemplateActionOutcome::Submitted => {
                    Ok(CommandResult::new(result_json(&path, &template, &result)))
                }
                TemplateActionOutcome::Blocked | TemplateActionOutcome::Invalid => {
                    Err(refusal(&file, &result))
                }
                TemplateActionOutcome::Ready => Err(unexpected_outcome(&file, &result)),
            }
        },
    )
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn result(value: serde_json::Value) -> TemplateActionResult {
        serde_json::from_value(value).expect("result fixture")
    }

    fn fix_of(err: &cli_engine::CliCoreError) -> String {
        cli_engine::build_error_envelope(err, SYSTEM)
            .fix
            .unwrap_or_default()
    }

    #[test]
    fn a_refusal_keeps_every_issue_with_its_code_field_and_fix() {
        let invalid = result(json!({
            "action": "SUBMIT", "outcome": "INVALID", "operation": "ADD",
            "issues": [
                { "code": "DCTL1012", "level": "ERROR", "message": "syncPubKeyDomain is required",
                  "field": "syncPubKeyDomain", "fix": "Add syncPubKeyDomain" },
                { "code": "SCHEMA", "level": "ERROR", "message": "records[0].pointsTo is required" }
            ]
        }));

        let err = refusal("acme.com.email.json", &invalid);

        let message = err.to_string();
        assert!(
            message.contains("acme.com.email.json was not submitted: INVALID"),
            "{message}"
        );
        assert!(
            message.contains("ERROR DCTL1012: syncPubKeyDomain is required"),
            "{message}"
        );
        assert!(message.contains("field: syncPubKeyDomain"), "{message}");
        assert!(message.contains("fix: Add syncPubKeyDomain"), "{message}");
        assert!(
            message.contains("ERROR SCHEMA: records[0].pointsTo is required"),
            "{message}"
        );
        assert!(fix_of(&err).contains("READY"), "{}", fix_of(&err));

        let envelope = cli_engine::build_error_envelope(&err, SYSTEM);
        assert_eq!(
            envelope.error.as_ref().map(|error| error.code.as_str()),
            Some("VALIDATION_ERROR")
        );
        assert_eq!(envelope.next_actions.len(), 1);
        assert_eq!(
            envelope.next_actions[0].command,
            "gddy domain-connect template validate <file>"
        );
        assert_eq!(
            envelope.next_actions[0].params["file"].value.as_deref(),
            Some("acme.com.email.json")
        );
    }

    #[test]
    fn a_blocked_refusal_points_at_ownership_and_version() {
        let blocked = result(json!({
            "action": "SUBMIT", "outcome": "BLOCKED", "operation": "MODIFY",
            "issues": [{ "code": "VERSION_NOT_INCREASED", "level": "ERROR",
                         "message": "version 1 is not greater than 1", "field": "version" }]
        }));

        let fix = fix_of(&refusal("t.json", &blocked));

        assert!(fix.contains("raise the template's version"), "{fix}");
        assert!(fix.contains("GoDaddy support"), "{fix}");
    }

    #[test]
    fn submit_handles_dry_run_itself_and_asks_for_both_permissions() {
        let meta = command().spec.metadata();
        assert!(meta.dry_run_prompt);
        assert!(meta.handles_dry_run);
        assert_eq!(
            meta.scopes,
            [DOMAIN_CONNECT_TEMPLATE_READ, DOMAIN_CONNECT_TEMPLATE_WRITE]
        );
    }

    #[test]
    fn help_tells_assistants_to_confirm_first() {
        let long = command().spec.long.unwrap_or_default();
        assert!(long.contains("AI assistants:"), "{long}");
        assert!(long.contains("--dry-run"), "{long}");
    }
}
