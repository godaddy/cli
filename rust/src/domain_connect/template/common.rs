//! Shared pieces of `template validate` and `template submit`: reading the
//! template file as written, calling the API, and shaping the result.

use std::path::{Path, PathBuf};

use cli_engine::TableColumn;
use domains_client::types::{
    TemplateActionOutcome, TemplateActionResult, TemplateSubmissionInput,
    TemplateValidationCriteria,
};
use serde_json::{Map, Value, json};

use crate::error::GddyError;

pub(super) const SYSTEM: &str = "domain-connect";

/// What the two checking commands tell an AI assistant about submitting.
pub(super) const SUBMIT_CONFIRMATION_NOTE: &str = "AI assistants: submitting publishes the template in a public pull request \
     and claims the provider ID for the signed-in account. Show the user the \
     file and the provider and service IDs, and get their explicit \
     confirmation before running `template submit`. Do not infer it from a \
     request to create or check a template.";

type Template = Map<String, Value>;

/// Reads the template file as a JSON object without decoding it into a typed
/// struct, so every field the user wrote reaches the API as written.
pub(super) fn read_template(file: &str) -> cli_engine::Result<(PathBuf, Template)> {
    let path = Path::new(file);
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let shown = absolute.display();
    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(
                GddyError::not_found(format!("Template file not found: {shown}"))
                    .with_fix(
                        "Run this command from the directory that holds the template, pass its \
                     full path, or create one with `gddy domain-connect template init`.",
                    )
                    .with_system(SYSTEM)
                    .into_cli_error(),
            );
        }
        Err(error) => {
            return Err(
                GddyError::validation(format!("Could not read {shown}: {error}"))
                    .with_fix("Check that the path is a file you can read.")
                    .with_system(SYSTEM)
                    .into_cli_error(),
            );
        }
    };
    let invalid = |message: String| {
        GddyError::validation(message)
            .with_fix("Fix the file, or write a new one with `gddy domain-connect template init`.")
            .with_system(SYSTEM)
            .into_cli_error()
    };
    match serde_json::from_str::<Value>(&content) {
        Ok(Value::Object(template)) => Ok((absolute, template)),
        Ok(_) => Err(invalid(format!(
            "{shown} must contain a JSON object (a Domain Connect template)."
        ))),
        Err(error) => Err(invalid(format!("{shown} is not valid JSON: {error}"))),
    }
}

/// `POST /v2/domains/validate-domain-connect-template`. Always `200` for a
/// template that can be processed; the verdict is in `outcome`.
pub(super) async fn validate(
    client: &domains_client::Client,
    template: &Template,
) -> cli_engine::Result<TemplateActionResult> {
    let body = TemplateValidationCriteria {
        template: template.clone(),
    };
    match client
        .validate_domain_connect_template()
        .body(body)
        .send()
        .await
    {
        Ok(response) => Ok(response.into_inner()),
        Err(error) => Err(api_error(error, false).await),
    }
}

/// `POST /v2/domains/submit-domain-connect-template`. `200` carries a
/// submitted result; `409` and `422` carry the same shape with the reasons.
pub(super) async fn submit(
    client: &domains_client::Client,
    template: &Template,
) -> cli_engine::Result<TemplateActionResult> {
    let body = TemplateSubmissionInput {
        template: template.clone(),
    };
    match client
        .submit_domain_connect_template()
        .body(body)
        .send()
        .await
    {
        Ok(response) => Ok(response.into_inner()),
        Err(domains_client::Error::UnexpectedResponse(response))
            if matches!(response.status().as_u16(), 409 | 422) =>
        {
            let status = response.status().as_u16();
            let bytes = response
                .bytes()
                .await
                .map_err(|error| network(error.to_string()))?;
            match serde_json::from_slice::<TemplateActionResult>(&bytes) {
                // A refusal has to say it is one: a body that claims success
                // on a 409 or 422 is not a result the caller can act on.
                Ok(result)
                    if matches!(
                        result.outcome,
                        TemplateActionOutcome::Blocked | TemplateActionOutcome::Invalid
                    ) =>
                {
                    Ok(result)
                }
                _ => Err(http_error(status, &String::from_utf8_lossy(&bytes), true)),
            }
        }
        Err(error) => Err(api_error(error, true).await),
    }
}

async fn api_error(error: domains_client::Error<()>, submitting: bool) -> cli_engine::CliCoreError {
    match error {
        domains_client::Error::UnexpectedResponse(response) => {
            let status = response.status().as_u16();
            let body = response.text().await.unwrap_or_default();
            http_error(status, &body, submitting)
        }
        domains_client::Error::CommunicationError(error) => network(error.to_string()),
        other => GddyError::unexpected(format!(
            "The template service returned a response this version of gddy cannot read: {other}"
        ))
        .with_fix("Update gddy with `gddy update apply`, then try again.")
        .with_system(SYSTEM)
        .into_cli_error(),
    }
}

fn network(message: String) -> cli_engine::CliCoreError {
    GddyError::network(message)
        .with_system(SYSTEM)
        .into_cli_error()
}

fn http_error(status: u16, body: &str, submitting: bool) -> cli_engine::CliCoreError {
    let error = GddyError::from_http(status, describe_error_body(body), SYSTEM);
    let error = match status {
        400 | 422 => error.with_fix(
            "Check that the file is a Domain Connect template: a JSON object with providerId, \
             providerName, serviceId, serviceName and a records list.",
        ),
        403 if submitting => error.with_fix(
            "Your account is not allowed to submit Domain Connect templates. Run `gddy auth \
             login` to sign in again, then retry.",
        ),
        403 => error.with_fix(
            "Your account is not allowed to check Domain Connect templates. Run `gddy auth \
             login` to sign in again, then retry.",
        ),
        429 => error.with_fix("Too many requests. Wait a minute, then try again."),
        _ => error,
    };
    error.into_cli_error()
}

/// The service's error body is `{name, message, details: [{field, issue,
/// description}]}`; anything else is passed through as text.
fn describe_error_body(body: &str) -> String {
    let Ok(Value::Object(error)) = serde_json::from_str::<Value>(body) else {
        return body.trim().to_owned();
    };
    fn text(value: Option<&Value>) -> Option<&str> {
        value.and_then(Value::as_str).filter(|s| !s.is_empty())
    }
    let mut lines = Vec::new();
    match (text(error.get("name")), text(error.get("message"))) {
        (Some(name), Some(message)) => lines.push(format!("{name}: {message}")),
        (Some(only), None) | (None, Some(only)) => lines.push(only.to_owned()),
        (None, None) => return body.trim().to_owned(),
    }
    for detail in error
        .get("details")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let parts: Vec<&str> = ["field", "issue", "description"]
            .iter()
            .filter_map(|key| text(detail.get(*key)))
            .collect();
        if !parts.is_empty() {
            lines.push(format!("  {}", parts.join(" ")));
        }
    }
    lines.join("\n")
}

/// The JSON a command returns: the API result plus the file it came from.
pub(super) fn result_json(
    file: &Path,
    template: &Template,
    result: &TemplateActionResult,
) -> Value {
    let mut out = json!({
        "file": file.display().to_string(),
        "providerId": template.get("providerId").cloned().unwrap_or(Value::Null),
        "serviceId": template.get("serviceId").cloned().unwrap_or(Value::Null),
        "action": result.action.to_string(),
        "outcome": result.outcome.to_string(),
        "operation": result.operation.map(|operation| operation.to_string()),
        "issues": result.issues.iter().map(|issue| json!({
            "level": issue.level.to_string(),
            "code": issue.code,
            "field": issue.field,
            "message": issue.message,
            "fix": issue.fix,
        })).collect::<Vec<_>>(),
    });
    if let Some(pr) = &result.pr {
        out["pullRequest"] = json!({
            "number": pr.number,
            "url": pr.url,
            "state": pr.state.map(|state| state.to_string()),
        });
    }
    out
}

/// What is wrong and how to fix it are never hidden or cut short, even on a
/// narrow terminal; the field and the code give way first.
fn issue_columns() -> Vec<TableColumn> {
    vec![
        TableColumn::new("level", "Level").essential(true),
        TableColumn::new("message", "Message").essential(true),
        TableColumn::new("fix", "Fix").essential(true),
        TableColumn::new("field", "Field"),
        TableColumn::new("code", "Code"),
    ]
}

fn result_columns() -> Vec<TableColumn> {
    vec![
        TableColumn::new("file", "File").no_truncate(true),
        TableColumn::new("outcome", "Outcome").essential(true),
        TableColumn::new("operation", "Operation"),
    ]
}

pub(super) fn validate_columns() -> Vec<TableColumn> {
    let mut columns = result_columns();
    columns.push(TableColumn::new("issues", "Issues").nested(issue_columns()));
    columns
}

pub(super) fn submit_columns() -> Vec<TableColumn> {
    let mut columns = result_columns();
    columns.push(TableColumn::new("pullRequest", "Pull request").nested(vec![
        TableColumn::new("url", "URL").no_truncate(true),
        TableColumn::new("state", "State"),
    ]));
    columns.push(TableColumn::new("issues", "Issues").nested(issue_columns()));
    columns
}

#[cfg(test)]
#[path = "common_tests.rs"]
mod tests;
