use cli_engine::{CommandResult, CommandSpec, NextActionParam, RuntimeCommandSpec, Tier};

use crate::hosting::nodejs::{client_err, make_client};
use crate::next_action::next_action;
use crate::scopes::HOSTING_CODE_WRITE as CODE_WRITE;

#[derive(Debug, Clone, clap::Args)]
struct SourceUploadArgs {
    /// Application ID.
    #[arg(long = "app-id", value_name = "APP_ID")]
    app_id: String,

    /// Path to the zip file.
    #[arg(long, short = 'f', value_name = "PATH")]
    file: String,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<SourceUploadArgs, _, _, _>(
        CommandSpec::from_args::<SourceUploadArgs>(
            "upload",
            "Upload a zip of source code to an app's preview environment",
        )
        .with_long(
            "Upload a zip of source code to an existing app. When the job \
            completes, the code is automatically deployed to the app's preview \
            environment. Use this whenever you want to update the app's code — \
            do not create a new app for each update. After upload completes, run \
            `deployment publish` to promote preview to live.",
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
