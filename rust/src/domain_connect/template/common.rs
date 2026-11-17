//! Shared pieces of `template validate` and `template submit`: reading the
//! template file as opaque JSON, building the client, and rendering an
//! `ActionResult` for human output.

use std::path::{Path, PathBuf};

use cli_engine::CommandContext;
use serde_json::{Value, json};

use super::client::{ActionResult, TemplatesClient, outcome};
use crate::error::GddyError;

/// Reads the template file as JSON without decoding it into a typed struct, so
/// every field the user wrote reaches the API as written.
pub(super) fn read_template(file: &str) -> cli_engine::Result<(PathBuf, Value)> {
    let path = Path::new(file);
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    };
    if !path.exists() {
        return Err(GddyError::not_found(format!(
            "Template file not found: {}",
            absolute.display()
        ))
        .with_fix(
            "Run this command from the directory containing the template, or pass its full path.",
        )
        .into_cli_error());
    }
    let content = std::fs::read_to_string(path).map_err(|e| {
        GddyError::unexpected(format!("Failed to read {}: {e}", absolute.display()))
            .into_cli_error()
    })?;
    let template: Value = serde_json::from_str(&content).map_err(|e| {
        GddyError::validation(format!("{} is not valid JSON: {e}", absolute.display()))
            .with_fix("Fix the JSON syntax, or regenerate the file with `gddy domain-connect template init`.")
            .into_cli_error()
    })?;
    if !template.is_object() {
        return Err(GddyError::validation(format!(
            "{} must contain a JSON object (a Domain Connect template)",
            absolute.display()
        ))
        .into_cli_error());
    }
    Ok((absolute, template))
}

pub(super) async fn make_client(ctx: &CommandContext) -> cli_engine::Result<TemplatesClient> {
    let token = ctx.credential().await?.token;
    let base_url = crate::http::api_url_for_env(&ctx.middleware.env)?;
    Ok(TemplatesClient::new(base_url, token))
}

/// The JSON the command returns: the API result plus the file it came from.
pub(super) fn result_json(file: &Path, template: &Value, result: &ActionResult) -> Value {
    let mut out = json!({
        "file": file.display().to_string(),
        "providerId": template.get("providerId").cloned().unwrap_or(Value::Null),
        "serviceId": template.get("serviceId").cloned().unwrap_or(Value::Null),
        "action": result.action,
        "outcome": result.outcome,
        "operation": result.operation,
        "issues": result.issues.iter().map(|i| json!({
            "code": i.code,
            "level": i.level,
            "message": i.message,
            "field": i.field,
            "fix": i.fix,
        })).collect::<Vec<_>>(),
    });
    if let Some(pr) = &result.pr {
        out["pr"] = json!({ "number": pr.number, "url": pr.url, "state": pr.state });
    }
    out
}

/// Lines for the human renderer: one per issue, `level code: message`, with
/// `field` and `fix` indented beneath when present.
pub(super) fn issue_lines(result: &ActionResult) -> Vec<String> {
    let mut lines = Vec::new();
    for issue in &result.issues {
        lines.push(format!("{} {}: {}", issue.level, issue.code, issue.message));
        if let Some(field) = &issue.field {
            lines.push(format!("    field: {field}"));
        }
        if let Some(fix) = &issue.fix {
            lines.push(format!("    fix:   {fix}"));
        }
    }
    lines
}

/// Human output for validate: the JSON result plus a `summary` the renderer
/// prints first. Issues are already structured in `issues`; the summary gives
/// the one-line verdict and the next step.
pub(super) fn human_validate(file: &Path, result: &ActionResult, mut data: Value) -> Value {
    let mut lines = vec![format!("{}: {}", file.display(), result.outcome)];
    match result.outcome.as_str() {
        outcome::READY => {
            lines.push("Ready to submit with `gddy domain-connect template submit`.".to_owned());
        }
        outcome::INVALID | outcome::BLOCKED => lines.extend(issue_lines(result)),
        _ => {}
    }
    data["summary"] = Value::String(lines.join("\n"));
    data
}

/// Human output for a successful submit: verdict, pull request URL and state.
pub(super) fn human_submit(file: &Path, result: &ActionResult, mut data: Value) -> Value {
    let mut lines = vec![format!("{}: {}", file.display(), result.outcome)];
    if let Some(pr) = &result.pr {
        if let Some(url) = &pr.url {
            lines.push(format!("Pull request: {url}"));
        }
        if let Some(state) = &pr.state {
            lines.push(format!("State:        {state}"));
        }
    }
    lines.push("Track review progress on the pull request.".to_owned());
    data["summary"] = Value::String(lines.join("\n"));
    data
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_template_keeps_unmodeled_fields_and_string_numbers() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("acme.com.svc.json");
        std::fs::write(
            &path,
            r#"{"providerId":"acme.com","warnPhishing":true,"futureField":{"a":1},"records":[{"type":"A","ttl":"%ttl%"}]}"#,
        )
        .expect("write");

        let (_, template) = read_template(path.to_str().expect("utf8")).expect("reads");

        assert_eq!(template["warnPhishing"], true);
        assert_eq!(template["futureField"]["a"], 1);
        assert_eq!(template["records"][0]["ttl"], "%ttl%");
    }

    #[test]
    fn read_template_rejects_a_non_object() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("list.json");
        std::fs::write(&path, "[1,2,3]").expect("write");
        let err = read_template(path.to_str().expect("utf8")).expect_err("not an object");
        assert!(err.to_string().contains("JSON object"), "{err}");
    }

    #[test]
    fn read_template_reports_missing_file() {
        let err = read_template("/definitely/not/here.json").expect_err("missing");
        assert!(err.to_string().contains("not found"), "{err}");
    }
}

#[cfg(test)]
mod render_tests {
    use std::path::Path;

    use serde_json::json;

    use super::super::client::{ActionResult, Issue, PullRequest};
    use super::*;

    fn invalid_result() -> ActionResult {
        ActionResult {
            action: "VALIDATE".to_owned(),
            outcome: "INVALID".to_owned(),
            operation: Some("ADD".to_owned()),
            issues: vec![Issue {
                code: "DCTL1012".to_owned(),
                level: "ERROR".to_owned(),
                message: "syncPubKeyDomain is required".to_owned(),
                field: Some("syncPubKeyDomain".to_owned()),
                fix: Some("Add syncPubKeyDomain".to_owned()),
            }],
            pr: None,
        }
    }

    #[test]
    fn human_validate_renders_verdict_and_issues() {
        let template = json!({"providerId": "acme.com", "serviceId": "svc"});
        let result = invalid_result();
        let data = result_json(Path::new("acme.com.svc.json"), &template, &result);
        let out = human_validate(Path::new("acme.com.svc.json"), &result, data);
        let rendered =
            cli_engine::render_human(&cli_engine::Envelope::success(out, "domain-connect"));
        assert!(
            rendered.contains("acme.com.svc.json: INVALID"),
            "{rendered}"
        );
        assert!(
            rendered.contains("ERROR DCTL1012: syncPubKeyDomain is required"),
            "{rendered}"
        );
        assert!(
            rendered.contains("fix:   Add syncPubKeyDomain"),
            "{rendered}"
        );
    }

    #[test]
    fn human_submit_renders_pull_request() {
        let template = json!({"providerId": "acme.com", "serviceId": "svc"});
        let result = ActionResult {
            action: "SUBMIT".to_owned(),
            outcome: "SUBMITTED".to_owned(),
            operation: Some("ADD".to_owned()),
            issues: vec![],
            pr: Some(PullRequest {
                number: Some(1234),
                url: Some("https://github.com/Domain-Connect/Templates/pull/1234".to_owned()),
                state: Some("FAST_TRACK".to_owned()),
            }),
        };
        let data = result_json(Path::new("acme.com.svc.json"), &template, &result);
        let out = human_submit(Path::new("acme.com.svc.json"), &result, data);
        let rendered =
            cli_engine::render_human(&cli_engine::Envelope::success(out, "domain-connect"));
        assert!(rendered.contains("SUBMITTED"), "{rendered}");
        assert!(rendered.contains("pull/1234"), "{rendered}");
        assert!(rendered.contains("FAST_TRACK"), "{rendered}");
    }

    #[test]
    fn result_json_carries_pr_only_when_present() {
        let template = json!({"providerId": "acme.com", "serviceId": "svc"});
        let no_pr = result_json(Path::new("f.json"), &template, &invalid_result());
        assert!(no_pr.get("pr").is_none());
        assert_eq!(no_pr["issues"][0]["code"], "DCTL1012");
        assert_eq!(no_pr["providerId"], "acme.com");
    }
}
