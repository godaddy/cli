use cli_engine::{CommandResult, CommandSpec, NextActionParam, RuntimeCommandSpec, Tier};
use serde_json::json;

use super::AppJobIdArgs;
use crate::hosting::nodejs::{client_err, is_source_job_terminal, make_client};
use crate::next_action::next_action;
use crate::scopes::{HOSTING_CODE_READ as CODE_READ, HOSTING_CODE_WRITE as CODE_WRITE};

#[derive(Debug, Clone, clap::Args)]
struct SourceGitArgs {
    /// Application ID.
    #[arg(long = "app-id", value_name = "APP_ID")]
    app_id: String,

    /// Branch to import.
    #[arg(long, value_name = "BRANCH")]
    branch: String,

    /// Repository in owner/repo format.
    #[arg(long, value_name = "OWNER/REPO")]
    repo: Option<String>,

    /// Repository clone URL.
    #[arg(long = "repo-url", value_name = "URL")]
    repo_url: Option<String>,

    /// Repository is private.
    #[arg(long)]
    private: bool,

    /// Clear working tree before import.
    #[arg(long)]
    clear: bool,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<SourceGitArgs, _, _, _>(
        CommandSpec::from_args::<SourceGitArgs>(
            "git",
            "Pull a GitHub branch into an app's preview environment",
        )
        .with_long(
            "Pull a branch from a connected GitHub repository into an existing app. \
                 When the job completes, the code is automatically deployed to preview. \
                 Use instead of `source upload` when the app's code lives in GitHub. \
                 After import completes, run `deployment publish` to promote to live.",
        )
        .with_system("hosting")
        .with_tier(Tier::Mutate)
        .mutates(true)
        .with_scopes(&[CODE_WRITE]),
        |ctx, args: SourceGitArgs| async move {
            let app_id = args.app_id;
            let branch = args.branch;
            let mut body = json!({ "branch": branch });
            // Treat an empty/whitespace-only value the same as an absent
            // flag, matching the pre-typed-args `optional_str` helper's
            // behavior — otherwise `--repo ""` would forward an invalid
            // empty field into the request body.
            if let Some(repo) = args.repo.filter(|s| !s.trim().is_empty()) {
                body["repoFullName"] = json!(repo);
            }
            if let Some(repo_url) = args.repo_url.filter(|s| !s.trim().is_empty()) {
                body["repoUrl"] = json!(repo_url);
            }
            if args.private {
                body["isPrivate"] = json!(true);
            }
            if args.clear {
                body["clearWorkingTree"] = json!(true);
            }
            let client = make_client(&ctx, &[CODE_WRITE]).await?;
            let data = client
                .start_git_import(&app_id, body)
                .await
                .map_err(client_err)?;
            let job_id = data.get("jobId").and_then(|v| v.as_str()).unwrap_or("");
            let action = if job_id.is_empty() {
                next_action(
                    "hosting nodejs source git-status --app-id <app-id> --job-id <job-id>",
                    "Poll git import status",
                )
                .with_param("app-id", NextActionParam::value(app_id))
                .with_param("job-id", NextActionParam::required())
            } else {
                next_action(
                    "hosting nodejs source git-status --app-id <app-id> --job-id <job-id>",
                    "Poll git import status",
                )
                .with_param("app-id", NextActionParam::value(app_id))
                .with_param("job-id", NextActionParam::value(job_id))
            };
            Ok(CommandResult::new(data).with_next_actions(vec![action]))
        },
    )
}

pub(super) fn status_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<AppJobIdArgs, _, _, _>(
        CommandSpec::from_args::<AppJobIdArgs>("git-status", "Poll git import job status")
            .with_long(
                "Poll a GitHub source import job until status is `complete` or `failed`. \
                 When `complete`, the source has been deployed to the app's preview \
                 environment; run `deployment publish` to promote to live.",
            )
            .with_system("hosting")
            .with_tier(Tier::Read)
            .with_scopes(&[CODE_READ]),
        |ctx, args: AppJobIdArgs| async move {
            let app_id = args.app_id;
            let job_id = args.job_id;
            let client = make_client(&ctx, &[CODE_READ]).await?;
            let data = client
                .get_git_import_status(&app_id, &job_id)
                .await
                .map_err(client_err)?;
            let status = data.get("status").and_then(|v| v.as_str()).unwrap_or("");
            let mut actions = Vec::new();
            if !is_source_job_terminal(status) {
                actions.push(
                    next_action(
                        "hosting nodejs source git-status --app-id <app-id> --job-id <job-id>",
                        "Re-check whether the import has finished",
                    )
                    .with_param("app-id", NextActionParam::value(app_id.clone()))
                    .with_param("job-id", NextActionParam::value(job_id)),
                );
            } else if status == "complete" {
                actions.push(
                    next_action(
                        "hosting nodejs deployment publish --app-id <app-id>",
                        "Publish after import completes",
                    )
                    .with_param("app-id", NextActionParam::value(app_id)),
                );
            }
            Ok(CommandResult::new(data).with_next_actions(actions))
        },
    )
}
