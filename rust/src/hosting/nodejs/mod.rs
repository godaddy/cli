pub mod client;

mod app;
mod deployment;
mod github;
mod job;
mod secrets;
mod source;

use cli_engine::{
    CliCoreError, CommandContext, CommandResult, CommandSpec, GroupSpec, NextActionParam, Result,
    RuntimeCommandSpec, RuntimeGroupSpec, Tier,
};

use crate::next_action::next_action;
use crate::{
    application::client::api_url_for_env, hosting::nodejs::client::HostingClient,
    output_schema::output_schema,
};

use crate::scopes::{HOSTING_APPS_READ as APPS_READ, HOSTING_LOGS_READ as LOGS_READ};

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

#[derive(Debug, Clone, clap::Args)]
struct AppIdArgs {
    /// Application ID.
    #[arg(long = "app-id", value_name = "APP_ID")]
    app_id: String,
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
    .with_group(app::group())
    .with_group(job::group())
    .with_group(deployment::group())
    .with_command(status_command())
    .with_group(source::group())
    .with_group(github::group())
    .with_group(secrets::group())
    .with_command(logs_command())
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
                "Fetch logs for a Node.js hosting app. `--target preview` or `publish` \
                 selects which environment. `stdout` and `stderr` are the running app's \
                 output; `exec` covers platform activity (npm install, build output, \
                 import failures) — use `exec` to help diagnose a failed deploy.",
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
    use super::{is_app_creation_job_terminal, is_source_job_terminal};

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
}
