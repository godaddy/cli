pub mod client;

use cli_engine::{
    CliCoreError, CommandContext, CommandResult, CommandSpec, GroupSpec, NextActionParam, Result,
    RuntimeCommandSpec, RuntimeGroupSpec, Tier,
};
use serde_json::{Value, json};

use crate::next_action::next_action;
use crate::{
    application::client::api_url_for_env, hosting::nodejs::client::HostingClient,
    output_schema::output_schema,
};

use crate::scopes::{
    HOSTING_APPS_CREATE as APPS_CREATE, HOSTING_APPS_DELETE as APPS_DELETE,
    HOSTING_APPS_READ as APPS_READ, HOSTING_APPS_UPDATE as APPS_UPDATE,
    HOSTING_CODE_READ as CODE_READ, HOSTING_CODE_WRITE as CODE_WRITE,
    HOSTING_DEPLOY_EXECUTE as DEPLOY_EXECUTE, HOSTING_GITHUB_EXECUTE as GITHUB_EXECUTE,
    HOSTING_LOGS_READ as LOGS_READ, HOSTING_SECRETS_WRITE as SECRETS_WRITE,
};

output_schema!(HostingAppSummary {
    "id": "string";
    "name": "string";
    "status": "string";
});

output_schema!(HostingJobSummary {
    "id": "string";
    "status": "string";
});

async fn make_client(ctx: &CommandContext, scopes: &[&str]) -> Result<HostingClient> {
    let required: Vec<String> = scopes.iter().map(|s| (*s).to_owned()).collect();
    let token = ctx.credential_with_scopes(&required).await?.token;
    let base_url = api_url_for_env(&ctx.middleware.env)?;
    Ok(HostingClient::new(base_url, token))
}

fn client_err(e: client::ClientError) -> CliCoreError {
    crate::error::GddyError::from(e).into_cli_error()
}

/// Whether a zip-upload or git-import job status is terminal (no further polling).
fn is_source_job_terminal(status: &str) -> bool {
    matches!(status, "complete" | "failed")
}

/// Whether an app-creation job status is terminal. `active` means the app is
/// provisioned (success), not that work is still running.
fn is_app_creation_job_terminal(status: &str) -> bool {
    matches!(status, "active" | "failed")
}

pub fn nodejs_group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(
        GroupSpec::new("nodejs", "Manage Node.js hosting applications").with_long(
            "Work with Node.js PaaS applications on GoDaddy hosting.\n\
             \n\
             Create apps, upload source, publish deployments, manage secrets, and \
             read logs. Async operations return job IDs — use the matching status \
             commands to poll until complete.",
        ),
    )
    .with_group(app_group())
    .with_group(job_group())
    .with_group(deployment_group())
    .with_command(status_command())
    .with_group(source_group())
    .with_group(github_group())
    .with_group(secrets_group())
    .with_command(logs_command())
}

fn app_group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(GroupSpec::new("app", "Manage hosting applications"))
        .with_command(app_list_command())
        .with_command(app_get_command())
        .with_command(app_create_command())
        .with_command(app_update_command())
        .with_command(app_delete_command())
}

fn job_group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(GroupSpec::new("job", "Poll app creation jobs"))
        .with_command(job_get_command())
}

fn deployment_group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(GroupSpec::new("deployment", "Manage app deployments"))
        .with_command(deployment_list_command())
        .with_command(deployment_publish_command())
}

fn source_group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(GroupSpec::new("source", "Upload and track app source"))
        .with_command(source_upload_command())
        .with_command(source_status_command())
        .with_command(source_git_command())
        .with_command(source_git_status_command())
}

fn github_group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(GroupSpec::new("github", "Manage GitHub connections"))
        .with_command(github_status_command())
        .with_command(github_repos_command())
        .with_command(github_branches_command())
}

fn secrets_group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(GroupSpec::new("secrets", "Manage app secrets"))
        .with_command(secrets_list_command())
        .with_command(secrets_update_command())
}

fn app_list_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_with_context(
        CommandSpec::new("list", "List Node.js hosting applications")
            .with_long("List all Node.js hosting applications in your account.")
            .with_system("hosting")
            .with_tier(Tier::Read)
            .with_scopes(&[APPS_READ])
            .with_default_fields("id,name,status")
            .with_output_schema::<HostingAppSummary>(),
        |ctx| async move {
            let client = make_client(&ctx, &[APPS_READ]).await?;
            let data = client.list_apps().await.map_err(client_err)?;
            Ok(CommandResult::new(data).with_next_actions(vec![
                next_action(
                    "hosting nodejs app get --app-id <app-id>",
                    "Get details for an application",
                )
                .with_param("app-id", NextActionParam::required()),
            ]))
        },
    )
}

#[derive(Debug, Clone, clap::Args)]
struct AppIdArgs {
    /// Application ID.
    #[arg(long = "app-id", value_name = "APP_ID")]
    app_id: String,
}

fn app_get_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<AppIdArgs, _, _, _>(
        CommandSpec::from_args::<AppIdArgs>("get", "Get a Node.js hosting application")
            .with_long("Get details for one Node.js hosting application.")
            .with_system("hosting")
            .with_tier(Tier::Read)
            .with_scopes(&[APPS_READ]),
        |ctx, args: AppIdArgs| async move {
            let app_id = args.app_id;
            let client = make_client(&ctx, &[APPS_READ]).await?;
            let data = client.get_app(&app_id).await.map_err(client_err)?;
            Ok(CommandResult::new(data).with_next_actions(vec![
                next_action(
                    "hosting nodejs deployment list --app-id <app-id>",
                    "List deployments for this application",
                )
                .with_param("app-id", NextActionParam::value(app_id.clone())),
                next_action(
                    "hosting nodejs status --app-id <app-id>",
                    "Get application status",
                )
                .with_param("app-id", NextActionParam::value(app_id)),
            ]))
        },
    )
}

#[derive(Debug, Clone, clap::Args)]
struct AppCreateArgs {
    /// Application name.
    #[arg(long, value_name = "NAME")]
    name: String,

    /// Datacenter (p3 or sxb1).
    #[arg(long, value_name = "DATACENTER", value_parser = ["p3", "sxb1"])]
    datacenter: Option<String>,
}

fn app_create_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<AppCreateArgs, _, _, _>(
        CommandSpec::from_args::<AppCreateArgs>("create", "Create a Node.js hosting application")
            .with_long(
                "Create a Node.js hosting application. Returns a creation job; poll \
                 `hosting nodejs job get` until the job is active.",
            )
            .with_system("hosting")
            .with_tier(Tier::Mutate)
            .mutates(true)
            .with_scopes(&[APPS_CREATE]),
        |ctx, args: AppCreateArgs| async move {
            let name = args.name;
            let mut body = json!({ "name": name });
            if let Some(datacenter) = args.datacenter {
                body["datacenter"] = json!(datacenter);
            }
            let client = make_client(&ctx, &[APPS_CREATE]).await?;
            let data = client.create_app(body).await.map_err(client_err)?;
            let job_id = data
                .pointer("/job/id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let mut actions = vec![
                next_action(
                    "hosting nodejs job get --job-id <job-id>",
                    "Poll app creation status",
                )
                .with_param("job-id", NextActionParam::required()),
            ];
            if !job_id.is_empty() {
                actions[0] = next_action(
                    "hosting nodejs job get --job-id <job-id>",
                    "Poll app creation status",
                )
                .with_param("job-id", NextActionParam::value(job_id));
            }
            Ok(CommandResult::new(data).with_next_actions(actions))
        },
    )
}

#[derive(Debug, Clone, clap::Args)]
struct AppUpdateArgs {
    /// Application ID.
    #[arg(long = "app-id", value_name = "APP_ID")]
    app_id: String,

    /// New application name.
    #[arg(long, value_name = "NAME")]
    name: Option<String>,

    /// Application root path.
    #[arg(long = "root-path", value_name = "PATH")]
    root_path: Option<String>,
}

fn app_update_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<AppUpdateArgs, _, _, _>(
        CommandSpec::from_args::<AppUpdateArgs>(
            "update",
            "Update Node.js hosting application metadata",
        )
        .with_long("Update application metadata (name and/or root path).")
        .with_system("hosting")
        .with_tier(Tier::Mutate)
        .mutates(true)
        .with_scopes(&[APPS_UPDATE]),
        |ctx, args: AppUpdateArgs| async move {
            let app_id = args.app_id;
            // Treat an empty/whitespace-only value the same as an absent flag,
            // matching the pre-typed-args `optional_str` helper's behavior —
            // otherwise `--name ""` bypasses the "at least one of" check below
            // and would send an invalid patch body.
            let name = args.name.filter(|s| !s.trim().is_empty());
            let root_path = args.root_path.filter(|s| !s.trim().is_empty());
            if name.is_none() && root_path.is_none() {
                return Err(crate::error::GddyError::validation(
                    "at least one of --name or --root-path is required",
                )
                .into_cli_error());
            }
            let mut body = serde_json::Map::new();
            if let Some(name) = name {
                body.insert("name".to_owned(), json!(name));
            }
            if let Some(root_path) = root_path {
                body.insert("rootPath".to_owned(), json!(root_path));
            }
            let client = make_client(&ctx, &[APPS_UPDATE]).await?;
            let data = client
                .patch_app(&app_id, Value::Object(body))
                .await
                .map_err(client_err)?;
            Ok(CommandResult::new(data))
        },
    )
}

fn app_delete_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<AppIdArgs, _, _, _>(
        CommandSpec::from_args::<AppIdArgs>("delete", "Delete a Node.js hosting application")
            .with_long("Permanently delete a Node.js hosting application.")
            .with_system("hosting")
            .with_tier(Tier::Destructive)
            .mutates(true)
            .with_scopes(&[APPS_DELETE]),
        |ctx, args: AppIdArgs| async move {
            let app_id = args.app_id;
            let client = make_client(&ctx, &[APPS_DELETE]).await?;
            client.delete_app(&app_id).await.map_err(client_err)?;
            Ok(
                CommandResult::new(json!({ "deleted": true, "appId": app_id })).with_next_actions(
                    vec![next_action(
                        "hosting nodejs app list",
                        "List remaining applications",
                    )],
                ),
            )
        },
    )
}

#[derive(Debug, Clone, clap::Args)]
struct JobIdArgs {
    /// App creation job ID.
    #[arg(long = "job-id", value_name = "JOB_ID")]
    job_id: String,
}

fn job_get_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<JobIdArgs, _, _, _>(
        CommandSpec::from_args::<JobIdArgs>("get", "Get app creation job status")
            .with_long(
                "Poll an app creation job until status is active (app provisioned) or failed.",
            )
            .with_system("hosting")
            .with_tier(Tier::Read)
            .with_scopes(&[APPS_CREATE])
            .with_default_fields("id,status")
            .with_output_schema::<HostingJobSummary>(),
        |ctx, args: JobIdArgs| async move {
            let job_id = args.job_id;
            let client = make_client(&ctx, &[APPS_CREATE]).await?;
            let mut data = client
                .get_app_creation_status(&job_id)
                .await
                .map_err(client_err)?;
            promote_job_fields(&mut data);
            let status = data
                .pointer("/job/status")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let mut actions = Vec::new();
            if !is_app_creation_job_terminal(status) {
                actions.push(
                    next_action(
                        "hosting nodejs job get --job-id <job-id>",
                        "Re-check job status",
                    )
                    .with_param("job-id", NextActionParam::value(job_id)),
                );
            } else if status == "active"
                && let Some(app_id) = data.pointer("/app/id").and_then(|v| v.as_str())
            {
                actions.push(
                    next_action(
                        "hosting nodejs app get --app-id <app-id>",
                        "View the provisioned application",
                    )
                    .with_param("app-id", NextActionParam::value(app_id)),
                );
            }
            Ok(CommandResult::new(data).with_next_actions(actions))
        },
    )
}

fn promote_job_fields(data: &mut Value) {
    let job = data.get("job").cloned();
    if let (Some(Value::Object(job)), Some(obj)) = (job, data.as_object_mut()) {
        for key in ["id", "status"] {
            if let Some(v) = job.get(key) {
                obj.insert(key.to_owned(), v.clone());
            }
        }
    }
}

#[derive(Debug, Clone, clap::Args)]
struct DeploymentListArgs {
    /// Application ID.
    #[arg(long = "app-id", value_name = "APP_ID")]
    app_id: String,

    // Local override of the engine's global `--limit`, typed `i64` to match
    // it (see `domain::suggest::SuggestArgs::limit` for the full rationale —
    // a differently-typed override panics with a downcast mismatch reading
    // the root `ArgMatches`).
    #[arg(
        long,
        value_name = "N",
        help = "Maximum deployments to return (1-50)",
        allow_negative_numbers = true,
        value_parser = clap::value_parser!(i64).range(1..=50)
    )]
    limit: Option<i64>,
}

fn deployment_list_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<DeploymentListArgs, _, _, _>(
        CommandSpec::from_args::<DeploymentListArgs>("list", "List application deployments")
            .with_long("List deployments for a Node.js hosting application.")
            .with_system("hosting")
            .with_tier(Tier::Read)
            .with_scopes(&[APPS_READ]),
        |ctx, args: DeploymentListArgs| async move {
            let app_id = args.app_id;
            let limit = args.limit.and_then(|n| u32::try_from(n).ok());
            let client = make_client(&ctx, &[APPS_READ]).await?;
            let data = client
                .list_deployments(&app_id, limit)
                .await
                .map_err(client_err)?;
            Ok(CommandResult::new(data).with_next_actions(vec![
                next_action(
                    "hosting nodejs deployment publish --app-id <app-id>",
                    "Publish the latest source",
                )
                .with_param("app-id", NextActionParam::value(app_id)),
            ]))
        },
    )
}

fn deployment_publish_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<AppIdArgs, _, _, _>(
        CommandSpec::from_args::<AppIdArgs>("publish", "Publish application (deploy latest code)")
            .with_long(
                "Deploy the latest uploaded source for a Node.js hosting application. \
                 Poll `hosting nodejs status` and `hosting nodejs deployment list` for progress.",
            )
            .with_system("hosting")
            .with_tier(Tier::Mutate)
            .mutates(true)
            .with_scopes(&[DEPLOY_EXECUTE]),
        |ctx, args: AppIdArgs| async move {
            let app_id = args.app_id;
            let client = make_client(&ctx, &[DEPLOY_EXECUTE]).await?;
            let data = client.publish_app(&app_id).await.map_err(client_err)?;
            Ok(CommandResult::new(data).with_next_actions(vec![
                next_action(
                    "hosting nodejs status --app-id <app-id>",
                    "Check application status",
                )
                .with_param("app-id", NextActionParam::value(app_id.clone())),
                next_action(
                    "hosting nodejs deployment list --app-id <app-id>",
                    "List deployments",
                )
                .with_param("app-id", NextActionParam::value(app_id)),
            ]))
        },
    )
}

fn status_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<AppIdArgs, _, _, _>(
        CommandSpec::from_args::<AppIdArgs>("status", "Get application status")
            .with_long("Get runtime status for a Node.js hosting application.")
            .with_system("hosting")
            .with_tier(Tier::Read)
            .with_scopes(&[APPS_READ]),
        |ctx, args: AppIdArgs| async move {
            let app_id = args.app_id;
            let client = make_client(&ctx, &[APPS_READ]).await?;
            let data = client.get_app_status(&app_id).await.map_err(client_err)?;
            Ok(CommandResult::new(data).with_next_actions(vec![
                next_action(
                    "hosting nodejs deployment list --app-id <app-id>",
                    "List deployments for this application",
                )
                .with_param("app-id", NextActionParam::value(app_id.clone())),
                next_action(
                    "hosting nodejs logs --app-id <app-id>",
                    "View application logs",
                )
                .with_param("app-id", NextActionParam::value(app_id)),
            ]))
        },
    )
}

#[derive(Debug, Clone, clap::Args)]
struct SourceUploadArgs {
    /// Application ID.
    #[arg(long = "app-id", value_name = "APP_ID")]
    app_id: String,

    /// Path to the zip file.
    #[arg(long, short = 'f', value_name = "PATH")]
    file: String,
}

fn source_upload_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<SourceUploadArgs, _, _, _>(
        CommandSpec::from_args::<SourceUploadArgs>("upload", "Upload application source (zip)")
            .with_long(
                "Upload a zip archive of application source. Returns a job ID; poll \
                 `hosting nodejs source status` until complete.",
            )
            .with_system("hosting")
            .with_tier(Tier::Mutate)
            .mutates(true)
            .with_scopes(&[CODE_WRITE]),
        |ctx, args: SourceUploadArgs| async move {
            let app_id = args.app_id;
            let file = args.file;
            let path = std::path::Path::new(&file);
            if !path.is_file() {
                return Err(crate::error::GddyError::validation(format!(
                    "zip file not found: {file}"
                ))
                .into_cli_error());
            }
            let client = make_client(&ctx, &[CODE_WRITE]).await?;
            let data = client
                .upload_source(&app_id, path)
                .await
                .map_err(client_err)?;
            let upload_job_id = data.get("jobId").and_then(|v| v.as_str()).unwrap_or("");
            let action = if upload_job_id.is_empty() {
                next_action(
                    "hosting nodejs source status --app-id <app-id> --job-id <job-id>",
                    "Poll zip upload status",
                )
                .with_param("app-id", NextActionParam::value(app_id))
                .with_param("job-id", NextActionParam::required())
            } else {
                next_action(
                    "hosting nodejs source status --app-id <app-id> --job-id <job-id>",
                    "Poll zip upload status",
                )
                .with_param("app-id", NextActionParam::value(app_id))
                .with_param("job-id", NextActionParam::value(upload_job_id))
            };
            Ok(CommandResult::new(data).with_next_actions(vec![action]))
        },
    )
}

#[derive(Debug, Clone, clap::Args)]
struct AppJobIdArgs {
    /// Application ID.
    #[arg(long = "app-id", value_name = "APP_ID")]
    app_id: String,

    /// Job ID.
    #[arg(long = "job-id", value_name = "JOB_ID")]
    job_id: String,
}

fn source_status_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<AppJobIdArgs, _, _, _>(
        CommandSpec::from_args::<AppJobIdArgs>("status", "Poll zip upload status")
            .with_long("Poll the status of a zip source upload job.")
            .with_system("hosting")
            .with_tier(Tier::Read)
            .with_scopes(&[CODE_WRITE]),
        |ctx, args: AppJobIdArgs| async move {
            let app_id = args.app_id;
            let job_id = args.job_id;
            let client = make_client(&ctx, &[CODE_WRITE]).await?;
            let data = client
                .get_source_upload_status(&app_id, &job_id)
                .await
                .map_err(client_err)?;
            let status = data.get("status").and_then(|v| v.as_str()).unwrap_or("");
            let mut actions = Vec::new();
            if !is_source_job_terminal(status) {
                actions.push(
                    next_action(
                        "hosting nodejs source status --app-id <app-id> --job-id <job-id>",
                        "Re-check whether the upload has finished",
                    )
                    .with_param("app-id", NextActionParam::value(app_id.clone()))
                    .with_param("job-id", NextActionParam::value(job_id)),
                );
            } else if status == "complete" {
                actions.push(
                    next_action(
                        "hosting nodejs deployment publish --app-id <app-id>",
                        "Publish after upload completes",
                    )
                    .with_param("app-id", NextActionParam::value(app_id)),
                );
            }
            Ok(CommandResult::new(data).with_next_actions(actions))
        },
    )
}

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

fn source_git_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<SourceGitArgs, _, _, _>(
        CommandSpec::from_args::<SourceGitArgs>("git", "Import source from a GitHub repository")
            .with_long(
                "Import source code from a GitHub repository branch. Returns a job ID; \
                 poll `hosting nodejs source git-status` until complete, then publish.",
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

fn source_git_status_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<AppJobIdArgs, _, _, _>(
        CommandSpec::from_args::<AppJobIdArgs>("git-status", "Poll git import job status")
            .with_long("Poll the status of a GitHub source import job.")
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

fn github_status_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<AppIdArgs, _, _, _>(
        CommandSpec::from_args::<AppIdArgs>("status", "Show GitHub connection status")
            .with_long("Show whether GitHub is connected to a Node.js hosting application.")
            .with_system("hosting")
            .with_tier(Tier::Read)
            .with_scopes(&[GITHUB_EXECUTE]),
        |ctx, args: AppIdArgs| async move {
            let app_id = args.app_id;
            let client = make_client(&ctx, &[GITHUB_EXECUTE]).await?;
            let data = client
                .get_github_status(&app_id)
                .await
                .map_err(client_err)?;
            let connected = data
                .get("connected")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let mut actions = Vec::new();
            if connected {
                actions.push(
                    next_action(
                        "hosting nodejs github repos --app-id <app-id>",
                        "List accessible GitHub repositories",
                    )
                    .with_param("app-id", NextActionParam::value(app_id)),
                );
            }
            Ok(CommandResult::new(data).with_next_actions(actions))
        },
    )
}

#[derive(Debug, Clone, clap::Args)]
struct GithubReposArgs {
    /// Application ID.
    #[arg(long = "app-id", value_name = "APP_ID")]
    app_id: String,

    /// Repositories per page (1-100).
    #[arg(long = "per-page", value_name = "N", value_parser = clap::value_parser!(u32).range(1..=100))]
    per_page: Option<u32>,

    /// Sort order (created, updated, pushed, full_name).
    #[arg(long, value_name = "SORT", value_parser = ["created", "updated", "pushed", "full_name"])]
    sort: Option<String>,
}

fn github_repos_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<GithubReposArgs, _, _, _>(
        CommandSpec::from_args::<GithubReposArgs>("repos", "List accessible GitHub repositories")
            .with_long("List GitHub repositories accessible to the connected account.")
            .with_system("hosting")
            .with_tier(Tier::Read)
            .with_scopes(&[GITHUB_EXECUTE]),
        |ctx, args: GithubReposArgs| async move {
            let app_id = args.app_id;
            let per_page = args.per_page;
            let sort = args.sort;
            let client = make_client(&ctx, &[GITHUB_EXECUTE]).await?;
            let data = client
                .list_github_repos(&app_id, per_page, sort.as_deref())
                .await
                .map_err(client_err)?;
            Ok(CommandResult::new(data).with_next_actions(vec![
                next_action(
                    "hosting nodejs github branches --app-id <app-id> --owner <owner> --repo <repo>",
                    "List branches for a repository",
                )
                .with_param("app-id", NextActionParam::value(app_id))
                .with_param("owner", NextActionParam::required())
                .with_param("repo", NextActionParam::required()),
            ]))
        },
    )
}

#[derive(Debug, Clone, clap::Args)]
struct GithubBranchesArgs {
    /// Application ID.
    #[arg(long = "app-id", value_name = "APP_ID")]
    app_id: String,

    /// Repository owner (user or org).
    #[arg(long, value_name = "OWNER")]
    owner: String,

    /// Repository name.
    #[arg(long, value_name = "REPO")]
    repo: String,

    /// Branches per page (1-100).
    #[arg(long = "per-page", value_name = "N", value_parser = clap::value_parser!(u32).range(1..=100))]
    per_page: Option<u32>,
}

fn github_branches_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<GithubBranchesArgs, _, _, _>(
        CommandSpec::from_args::<GithubBranchesArgs>(
            "branches",
            "List branches for a GitHub repository",
        )
        .with_long("List branches for a GitHub repository accessible to the connected account.")
        .with_system("hosting")
        .with_tier(Tier::Read)
        .with_scopes(&[GITHUB_EXECUTE]),
        |ctx, args: GithubBranchesArgs| async move {
            let app_id = args.app_id;
            let owner = args.owner;
            let repo = args.repo;
            let per_page = args.per_page;
            let client = make_client(&ctx, &[GITHUB_EXECUTE]).await?;
            let data = client
                .list_github_branches(&app_id, &owner, &repo, per_page)
                .await
                .map_err(client_err)?;
            Ok(CommandResult::new(data).with_next_actions(vec![
                next_action(
                    "hosting nodejs source git --app-id <app-id> --repo <owner/repo> --branch <branch>",
                    "Import source from this repository",
                )
                .with_param("app-id", NextActionParam::value(app_id))
                .with_param("repo", NextActionParam::value(format!("{owner}/{repo}")))
                .with_param("branch", NextActionParam::required()),
            ]))
        },
    )
}

#[derive(Debug, Clone, clap::Args)]
struct SecretsListArgs {
    /// Application ID.
    #[arg(long = "app-id", value_name = "APP_ID")]
    app_id: String,

    /// Variant filter (preview or publish).
    #[arg(long, value_name = "VARIANT", value_parser = ["preview", "publish"])]
    variant: Option<String>,
}

fn secrets_list_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<SecretsListArgs, _, _, _>(
        CommandSpec::from_args::<SecretsListArgs>(
            "list",
            "List application secrets (metadata only)",
        )
        .with_long("List secret names and metadata. Secret values are never returned.")
        .with_system("hosting")
        .with_tier(Tier::Read)
        .with_scopes(&[SECRETS_WRITE]),
        |ctx, args: SecretsListArgs| async move {
            let app_id = args.app_id;
            let variant = args.variant;
            let client = make_client(&ctx, &[SECRETS_WRITE]).await?;
            let data = client
                .list_secrets(&app_id, variant.as_deref())
                .await
                .map_err(client_err)?;
            Ok(CommandResult::new(data))
        },
    )
}

#[derive(Debug, Clone, clap::Args)]
struct SecretsUpdateArgs {
    /// Application ID.
    #[arg(long = "app-id", value_name = "APP_ID")]
    app_id: String,

    /// JSON file with secret operations.
    #[arg(long, short = 'f', value_name = "PATH")]
    file: String,
}

fn secrets_update_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<SecretsUpdateArgs, _, _, _>(
        CommandSpec::from_args::<SecretsUpdateArgs>(
            "update",
            "Add, update, or delete application secrets",
        )
        .with_long("Update secrets from a JSON file matching the PublicSecretsUpdateBody schema.")
        .with_system("hosting")
        .with_tier(Tier::Mutate)
        .mutates(true)
        .with_scopes(&[SECRETS_WRITE]),
        |ctx, args: SecretsUpdateArgs| async move {
            let app_id = args.app_id;
            let file = args.file;
            let content = std::fs::read_to_string(&file).map_err(|e| {
                crate::error::GddyError::validation(format!(
                    "failed to read secrets file '{file}': {e}"
                ))
                .into_cli_error()
            })?;
            let body: Value = serde_json::from_str(&content).map_err(|e| {
                crate::error::GddyError::validation(format!(
                    "invalid JSON in secrets file '{file}': {e}"
                ))
                .into_cli_error()
            })?;
            let client = make_client(&ctx, &[SECRETS_WRITE]).await?;
            let data = client
                .update_secrets(&app_id, body)
                .await
                .map_err(client_err)?;
            Ok(CommandResult::new(data))
        },
    )
}

#[derive(Debug, Clone, clap::Args)]
struct LogsArgs {
    /// Application ID.
    #[arg(long = "app-id", value_name = "APP_ID")]
    app_id: String,

    /// Log target (preview or publish).
    #[arg(long, value_name = "TARGET", value_parser = ["preview", "publish"])]
    target: String,

    /// Log stream (stdout, stderr, or exec).
    #[arg(long, value_name = "SOURCE", value_parser = ["stdout", "stderr", "exec"])]
    source: String,

    /// Return logs since this ISO timestamp.
    #[arg(long, value_name = "TIMESTAMP")]
    since: String,

    /// Maximum log lines to return (1-500).
    #[arg(long, value_name = "N", value_parser = clap::value_parser!(u32).range(1..=500))]
    lines: Option<u32>,
}

fn logs_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<LogsArgs, _, _, _>(
        CommandSpec::from_args::<LogsArgs>("logs", "Get application logs")
            .with_long(
                "Fetch log entries for a Node.js hosting application. \
                 Choose --source to select which stream to read: `stdout` and \
                 `stderr` are the running app's own output, while `exec` \
                 covers platform activity around it (source import, npm \
                 install, and build output all land here, not in stderr).",
            )
            .with_system("hosting")
            .with_tier(Tier::Read)
            .with_scopes(&[LOGS_READ]),
        |ctx, args: LogsArgs| async move {
            let app_id = args.app_id;
            let target = args.target;
            let source = args.source;
            let since = args.since;
            let lines = args.lines;
            let client = make_client(&ctx, &[LOGS_READ]).await?;
            let data = client
                .get_logs(&app_id, &target, &source, &since, lines)
                .await
                .map_err(client_err)?;
            Ok(CommandResult::new(data))
        },
    )
}

#[cfg(test)]
mod tests {
    use super::{
        deployment_list_command, is_app_creation_job_terminal, is_source_job_terminal,
        promote_job_fields,
    };
    use serde_json::json;

    /// Builds a clap command from the real `--limit` arg rather than a hand-rolled copy.
    fn deployment_list_clap_command() -> clap::Command {
        clap::Command::new("list").args(deployment_list_command().spec.args)
    }

    #[test]
    fn limit_rejects_out_of_range_and_negative_values() {
        for bad in ["0", "51", "-1"] {
            let err = deployment_list_clap_command()
                .try_get_matches_from(["list", "--app-id", "app-1", "--limit", bad])
                .expect_err("out-of-range --limit should be rejected");
            assert_eq!(
                err.kind(),
                clap::error::ErrorKind::ValueValidation,
                "--limit {bad} should fail range validation, got: {err}"
            );
        }
    }

    #[test]
    fn limit_accepts_the_full_valid_range() {
        for good in ["1", "50"] {
            deployment_list_clap_command()
                .try_get_matches_from(["list", "--app-id", "app-1", "--limit", good])
                .expect("value within the valid --limit range should be accepted");
        }
    }

    #[test]
    fn source_job_terminal_status_detection() {
        assert!(is_source_job_terminal("complete"));
        assert!(is_source_job_terminal("failed"));
        assert!(!is_source_job_terminal("in_progress"));
        assert!(!is_source_job_terminal(""));
        assert!(!is_source_job_terminal("COMPLETE"));
    }

    #[test]
    fn app_creation_job_terminal_status_detection() {
        assert!(is_app_creation_job_terminal("active"));
        assert!(is_app_creation_job_terminal("failed"));
        assert!(!is_app_creation_job_terminal("pending"));
        assert!(!is_app_creation_job_terminal(""));
        assert!(!is_app_creation_job_terminal("ACTIVE"));
    }

    #[test]
    fn promote_job_fields_lifts_id_and_status_to_top() {
        let mut data = json!({
            "job": { "id": "job-1", "status": "active" },
            "app": { "id": "app-1", "name": "demo" },
        });
        promote_job_fields(&mut data);
        assert_eq!(data["id"], "job-1");
        assert_eq!(data["status"], "active");
        assert_eq!(data["job"]["id"], "job-1");
        assert_eq!(data["app"]["name"], "demo");
    }

    #[test]
    fn promote_job_fields_is_noop_without_job() {
        let mut data = json!({ "app": { "id": "app-1" } });
        promote_job_fields(&mut data);
        assert!(data.get("id").is_none());
        assert!(data.get("status").is_none());
        assert_eq!(data["app"]["id"], "app-1");
    }
}
