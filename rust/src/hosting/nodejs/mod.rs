pub mod client;

use cli_engine::{
    CliCoreError, CommandContext, CommandResult, CommandSpec, GroupSpec, NextAction,
    NextActionParam, Result, RuntimeCommandSpec, RuntimeGroupSpec, Tier,
};
use serde_json::{Value, json};

use crate::{
    application::client::api_url_for_env, hosting::nodejs::client::HostingClient,
    output_schema::output_schema,
};

use crate::scopes::{
    HOSTING_APPS_CREATE as APPS_CREATE, HOSTING_APPS_DELETE as APPS_DELETE,
    HOSTING_APPS_READ as APPS_READ, HOSTING_APPS_UPDATE as APPS_UPDATE,
    HOSTING_CODE_WRITE as CODE_WRITE, HOSTING_DEPLOY_EXECUTE as DEPLOY_EXECUTE,
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
    let base_url = api_url_for_env(&ctx.middleware.env);
    Ok(HostingClient::new(base_url, token))
}

fn client_err(e: client::ClientError) -> CliCoreError {
    CliCoreError::message(e.to_string())
}

fn required_str(ctx: &CommandContext, key: &str, label: &str) -> Result<String> {
    optional_str(ctx, key).ok_or_else(|| CliCoreError::message(format!("{label} is required")))
}

fn optional_str(ctx: &CommandContext, key: &str) -> Option<String> {
    ctx.args
        .get(key)
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

fn optional_u32(ctx: &CommandContext, key: &str) -> Option<u32> {
    let v = ctx.args.get(key)?;
    v.as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
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
                NextAction::new(
                    "hosting nodejs app get --app-id <app-id>",
                    "Get details for an application",
                )
                .with_param("app-id", NextActionParam::required()),
            ]))
        },
    )
}

fn app_get_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_with_context(
        CommandSpec::new("get", "Get a Node.js hosting application")
            .with_long("Get details for one Node.js hosting application.")
            .with_system("hosting")
            .with_tier(Tier::Read)
            .with_scopes(&[APPS_READ])
            .with_arg(
                clap::Arg::new("app-id")
                    .long("app-id")
                    .value_name("APP_ID")
                    .required(true)
                    .help("Application ID"),
            ),
        |ctx| async move {
            let app_id = required_str(&ctx, "app-id", "--app-id")?;
            let client = make_client(&ctx, &[APPS_READ]).await?;
            let data = client.get_app(&app_id).await.map_err(client_err)?;
            Ok(CommandResult::new(data).with_next_actions(vec![
                NextAction::new(
                    "hosting nodejs deployment list --app-id <app-id>",
                    "List deployments for this application",
                )
                .with_param("app-id", NextActionParam::value(app_id.clone())),
                NextAction::new(
                    "hosting nodejs status --app-id <app-id>",
                    "Get application status",
                )
                .with_param("app-id", NextActionParam::value(app_id)),
            ]))
        },
    )
}

fn app_create_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_with_context(
        CommandSpec::new("create", "Create a Node.js hosting application")
            .with_long(
                "Create a Node.js hosting application. Returns a creation job; poll \
                 `hosting nodejs job get` until the job is active.",
            )
            .with_system("hosting")
            .with_tier(Tier::Mutate)
            .mutates(true)
            .with_scopes(&[APPS_CREATE])
            .with_arg(
                clap::Arg::new("name")
                    .long("name")
                    .value_name("NAME")
                    .required(true)
                    .help("Application name"),
            )
            .with_arg(
                clap::Arg::new("datacenter")
                    .long("datacenter")
                    .value_name("DATACENTER")
                    .value_parser(["p3", "sxb1"])
                    .help("Datacenter (p3 or sxb1)"),
            ),
        |ctx| async move {
            let name = required_str(&ctx, "name", "--name")?;
            let mut body = json!({ "name": name });
            if let Some(datacenter) = optional_str(&ctx, "datacenter") {
                body["datacenter"] = json!(datacenter);
            }
            let client = make_client(&ctx, &[APPS_CREATE]).await?;
            let data = client.create_app(body).await.map_err(client_err)?;
            let job_id = data
                .pointer("/job/id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let mut actions = vec![
                NextAction::new(
                    "hosting nodejs job get --job-id <job-id>",
                    "Poll app creation status",
                )
                .with_param("job-id", NextActionParam::required()),
            ];
            if !job_id.is_empty() {
                actions[0] = NextAction::new(
                    "hosting nodejs job get --job-id <job-id>",
                    "Poll app creation status",
                )
                .with_param("job-id", NextActionParam::value(job_id));
            }
            Ok(CommandResult::new(data).with_next_actions(actions))
        },
    )
}

fn app_update_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_with_context(
        CommandSpec::new("update", "Update Node.js hosting application metadata")
            .with_long("Update application metadata (name and/or root path).")
            .with_system("hosting")
            .with_tier(Tier::Mutate)
            .mutates(true)
            .with_scopes(&[APPS_UPDATE])
            .with_arg(
                clap::Arg::new("app-id")
                    .long("app-id")
                    .value_name("APP_ID")
                    .required(true)
                    .help("Application ID"),
            )
            .with_arg(
                clap::Arg::new("name")
                    .long("name")
                    .value_name("NAME")
                    .help("New application name"),
            )
            .with_arg(
                clap::Arg::new("root-path")
                    .long("root-path")
                    .value_name("PATH")
                    .help("Application root path"),
            ),
        |ctx| async move {
            let app_id = required_str(&ctx, "app-id", "--app-id")?;
            let name = optional_str(&ctx, "name");
            let root_path = optional_str(&ctx, "root-path");
            if name.is_none() && root_path.is_none() {
                return Err(CliCoreError::message(
                    "at least one of --name or --root-path is required",
                ));
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
    RuntimeCommandSpec::new_with_context(
        CommandSpec::new("delete", "Delete a Node.js hosting application")
            .with_long("Permanently delete a Node.js hosting application.")
            .with_system("hosting")
            .with_tier(Tier::Destructive)
            .mutates(true)
            .with_scopes(&[APPS_DELETE])
            .with_arg(
                clap::Arg::new("app-id")
                    .long("app-id")
                    .value_name("APP_ID")
                    .required(true)
                    .help("Application ID"),
            ),
        |ctx| async move {
            let app_id = required_str(&ctx, "app-id", "--app-id")?;
            let client = make_client(&ctx, &[APPS_DELETE]).await?;
            client.delete_app(&app_id).await.map_err(client_err)?;
            Ok(
                CommandResult::new(json!({ "deleted": true, "appId": app_id })).with_next_actions(
                    vec![NextAction::new(
                        "hosting nodejs app list",
                        "List remaining applications",
                    )],
                ),
            )
        },
    )
}

fn job_get_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_with_context(
        CommandSpec::new("get", "Get app creation job status")
            .with_long(
                "Poll an app creation job until status is active (app provisioned) or failed.",
            )
            .with_system("hosting")
            .with_tier(Tier::Read)
            .with_scopes(&[APPS_CREATE])
            .with_default_fields("id,status")
            .with_output_schema::<HostingJobSummary>()
            .with_arg(
                clap::Arg::new("job-id")
                    .long("job-id")
                    .value_name("JOB_ID")
                    .required(true)
                    .help("App creation job ID"),
            ),
        |ctx| async move {
            let job_id = required_str(&ctx, "job-id", "--job-id")?;
            let client = make_client(&ctx, &[APPS_CREATE]).await?;
            let data = client
                .get_app_creation_status(&job_id)
                .await
                .map_err(client_err)?;
            let status = data
                .pointer("/job/status")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let mut actions = vec![
                NextAction::new(
                    "hosting nodejs job get --job-id <job-id>",
                    "Re-check job status",
                )
                .with_param("job-id", NextActionParam::value(job_id)),
            ];
            if let Some(app_id) = data.pointer("/app/id").and_then(|v| v.as_str()) {
                actions.push(
                    NextAction::new(
                        "hosting nodejs app get --app-id <app-id>",
                        "View the provisioned application",
                    )
                    .with_param("app-id", NextActionParam::value(app_id)),
                );
            } else if status == "pending" || status == "active" {
                // keep poll action only
            }
            Ok(CommandResult::new(data).with_next_actions(actions))
        },
    )
}

fn deployment_list_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_with_context(
        CommandSpec::new("list", "List application deployments")
            .with_long("List deployments for a Node.js hosting application.")
            .with_system("hosting")
            .with_tier(Tier::Read)
            .with_scopes(&[APPS_READ])
            .with_arg(
                clap::Arg::new("app-id")
                    .long("app-id")
                    .value_name("APP_ID")
                    .required(true)
                    .help("Application ID"),
            )
            .with_arg(
                clap::Arg::new("limit")
                    .long("limit")
                    .value_name("N")
                    .value_parser(clap::value_parser!(u32).range(1..=50))
                    .help("Maximum deployments to return (1-50)"),
            ),
        |ctx| async move {
            let app_id = required_str(&ctx, "app-id", "--app-id")?;
            let limit = optional_u32(&ctx, "limit");
            let client = make_client(&ctx, &[APPS_READ]).await?;
            let data = client
                .list_deployments(&app_id, limit)
                .await
                .map_err(client_err)?;
            Ok(CommandResult::new(data).with_next_actions(vec![
                NextAction::new(
                    "hosting nodejs deployment publish --app-id <app-id>",
                    "Publish the latest source",
                )
                .with_param("app-id", NextActionParam::value(app_id)),
            ]))
        },
    )
}

fn deployment_publish_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_with_context(
        CommandSpec::new("publish", "Publish application (deploy latest code)")
            .with_long(
                "Deploy the latest uploaded source for a Node.js hosting application. \
                 Poll `hosting nodejs status` and `hosting nodejs deployment list` for progress.",
            )
            .with_system("hosting")
            .with_tier(Tier::Mutate)
            .mutates(true)
            .with_scopes(&[DEPLOY_EXECUTE])
            .with_arg(
                clap::Arg::new("app-id")
                    .long("app-id")
                    .value_name("APP_ID")
                    .required(true)
                    .help("Application ID"),
            ),
        |ctx| async move {
            let app_id = required_str(&ctx, "app-id", "--app-id")?;
            let client = make_client(&ctx, &[DEPLOY_EXECUTE]).await?;
            let data = client.publish_app(&app_id).await.map_err(client_err)?;
            Ok(CommandResult::new(data).with_next_actions(vec![
                NextAction::new(
                    "hosting nodejs status --app-id <app-id>",
                    "Check application status",
                )
                .with_param("app-id", NextActionParam::value(app_id.clone())),
                NextAction::new(
                    "hosting nodejs deployment list --app-id <app-id>",
                    "List deployments",
                )
                .with_param("app-id", NextActionParam::value(app_id)),
            ]))
        },
    )
}

fn status_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_with_context(
        CommandSpec::new("status", "Get application status")
            .with_long("Get runtime status for a Node.js hosting application.")
            .with_system("hosting")
            .with_tier(Tier::Read)
            .with_scopes(&[APPS_READ])
            .with_arg(
                clap::Arg::new("app-id")
                    .long("app-id")
                    .value_name("APP_ID")
                    .required(true)
                    .help("Application ID"),
            ),
        |ctx| async move {
            let app_id = required_str(&ctx, "app-id", "--app-id")?;
            let client = make_client(&ctx, &[APPS_READ]).await?;
            let data = client.get_app_status(&app_id).await.map_err(client_err)?;
            Ok(CommandResult::new(data).with_next_actions(vec![
                NextAction::new(
                    "hosting nodejs deployment list --app-id <app-id>",
                    "List deployments for this application",
                )
                .with_param("app-id", NextActionParam::value(app_id.clone())),
                NextAction::new(
                    "hosting nodejs logs --app-id <app-id>",
                    "View application logs",
                )
                .with_param("app-id", NextActionParam::value(app_id)),
            ]))
        },
    )
}

fn source_upload_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_with_context(
        CommandSpec::new("upload", "Upload application source (zip)")
            .with_long(
                "Upload a zip archive of application source. Returns a job ID; poll \
                 `hosting nodejs source status` until complete.",
            )
            .with_system("hosting")
            .with_tier(Tier::Mutate)
            .mutates(true)
            .with_scopes(&[CODE_WRITE])
            .with_arg(
                clap::Arg::new("app-id")
                    .long("app-id")
                    .value_name("APP_ID")
                    .required(true)
                    .help("Application ID"),
            )
            .with_arg(
                clap::Arg::new("file")
                    .long("file")
                    .short('f')
                    .value_name("PATH")
                    .required(true)
                    .help("Path to the zip file"),
            ),
        |ctx| async move {
            let app_id = required_str(&ctx, "app-id", "--app-id")?;
            let file = required_str(&ctx, "file", "--file")?;
            let path = std::path::Path::new(&file);
            if !path.is_file() {
                return Err(CliCoreError::message(format!("zip file not found: {file}")));
            }
            let client = make_client(&ctx, &[CODE_WRITE]).await?;
            let data = client
                .upload_source(&app_id, path)
                .await
                .map_err(client_err)?;
            let upload_job_id = data.get("jobId").and_then(|v| v.as_str()).unwrap_or("");
            let action = if upload_job_id.is_empty() {
                NextAction::new(
                    "hosting nodejs source status --app-id <app-id> --job-id <job-id>",
                    "Poll zip upload status",
                )
                .with_param("app-id", NextActionParam::value(app_id))
                .with_param("job-id", NextActionParam::required())
            } else {
                NextAction::new(
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

fn source_status_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_with_context(
        CommandSpec::new("status", "Poll zip upload status")
            .with_long("Poll the status of a zip source upload job.")
            .with_system("hosting")
            .with_tier(Tier::Read)
            .with_scopes(&[CODE_WRITE])
            .with_arg(
                clap::Arg::new("app-id")
                    .long("app-id")
                    .value_name("APP_ID")
                    .required(true)
                    .help("Application ID"),
            )
            .with_arg(
                clap::Arg::new("job-id")
                    .long("job-id")
                    .value_name("JOB_ID")
                    .required(true)
                    .help("Upload job ID"),
            ),
        |ctx| async move {
            let app_id = required_str(&ctx, "app-id", "--app-id")?;
            let job_id = required_str(&ctx, "job-id", "--job-id")?;
            let client = make_client(&ctx, &[CODE_WRITE]).await?;
            let data = client
                .get_source_upload_status(&app_id, &job_id)
                .await
                .map_err(client_err)?;
            Ok(CommandResult::new(data).with_next_actions(vec![
                NextAction::new(
                    "hosting nodejs source status --app-id <app-id> --job-id <job-id>",
                    "Poll again while upload is in progress",
                )
                .with_param("app-id", NextActionParam::value(app_id.clone()))
                .with_param("job-id", NextActionParam::value(job_id)),
                NextAction::new(
                    "hosting nodejs deployment publish --app-id <app-id>",
                    "Publish after upload completes",
                )
                .with_param("app-id", NextActionParam::value(app_id)),
            ]))
        },
    )
}

fn secrets_list_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_with_context(
        CommandSpec::new("list", "List application secrets (metadata only)")
            .with_long("List secret names and metadata. Secret values are never returned.")
            .with_system("hosting")
            .with_tier(Tier::Read)
            .with_scopes(&[SECRETS_WRITE])
            .with_arg(
                clap::Arg::new("app-id")
                    .long("app-id")
                    .value_name("APP_ID")
                    .required(true)
                    .help("Application ID"),
            )
            .with_arg(
                clap::Arg::new("variant")
                    .long("variant")
                    .value_name("VARIANT")
                    .value_parser(["preview", "publish"])
                    .help("Variant filter (preview or publish)"),
            ),
        |ctx| async move {
            let app_id = required_str(&ctx, "app-id", "--app-id")?;
            let variant = optional_str(&ctx, "variant");
            let client = make_client(&ctx, &[SECRETS_WRITE]).await?;
            let data = client
                .list_secrets(&app_id, variant.as_deref())
                .await
                .map_err(client_err)?;
            Ok(CommandResult::new(data))
        },
    )
}

fn secrets_update_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_with_context(
        CommandSpec::new("update", "Add, update, or delete application secrets")
            .with_long(
                "Update secrets from a JSON file matching the PublicSecretsUpdateBody schema.",
            )
            .with_system("hosting")
            .with_tier(Tier::Mutate)
            .mutates(true)
            .with_scopes(&[SECRETS_WRITE])
            .with_arg(
                clap::Arg::new("app-id")
                    .long("app-id")
                    .value_name("APP_ID")
                    .required(true)
                    .help("Application ID"),
            )
            .with_arg(
                clap::Arg::new("file")
                    .long("file")
                    .short('f')
                    .value_name("PATH")
                    .required(true)
                    .help("JSON file with secret operations"),
            ),
        |ctx| async move {
            let app_id = required_str(&ctx, "app-id", "--app-id")?;
            let file = required_str(&ctx, "file", "--file")?;
            let content = std::fs::read_to_string(&file).map_err(|e| {
                CliCoreError::message(format!("failed to read secrets file '{file}': {e}"))
            })?;
            let body: Value = serde_json::from_str(&content).map_err(|e| {
                CliCoreError::message(format!("invalid JSON in secrets file '{file}': {e}"))
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

fn logs_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_with_context(
        CommandSpec::new("logs", "Get application logs")
            .with_long("Fetch log entries for a Node.js hosting application.")
            .with_system("hosting")
            .with_tier(Tier::Read)
            .with_scopes(&[LOGS_READ])
            .with_arg(
                clap::Arg::new("app-id")
                    .long("app-id")
                    .value_name("APP_ID")
                    .required(true)
                    .help("Application ID"),
            )
            .with_arg(
                clap::Arg::new("target")
                    .long("target")
                    .value_name("TARGET")
                    .required(true)
                    .value_parser(["preview", "publish"])
                    .help("Log target (preview or publish)"),
            )
            .with_arg(
                clap::Arg::new("source")
                    .long("source")
                    .value_name("SOURCE")
                    .required(true)
                    .help("Log source identifier"),
            )
            .with_arg(
                clap::Arg::new("since")
                    .long("since")
                    .value_name("TIMESTAMP")
                    .required(true)
                    .help("Return logs since this ISO timestamp"),
            )
            .with_arg(
                clap::Arg::new("lines")
                    .long("lines")
                    .value_name("N")
                    .value_parser(clap::value_parser!(u32).range(1..=500))
                    .help("Maximum log lines to return (1-500)"),
            ),
        |ctx| async move {
            let app_id = required_str(&ctx, "app-id", "--app-id")?;
            let target = required_str(&ctx, "target", "--target")?;
            let source = required_str(&ctx, "source", "--source")?;
            let since = required_str(&ctx, "since", "--since")?;
            let lines = optional_u32(&ctx, "lines");
            let client = make_client(&ctx, &[LOGS_READ]).await?;
            let data = client
                .get_logs(&app_id, &target, &source, &since, lines)
                .await
                .map_err(client_err)?;
            Ok(CommandResult::new(data))
        },
    )
}
