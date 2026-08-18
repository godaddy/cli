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

/// Ensure the given zip path exists as a file before attempting upload.
fn require_zip_file(file: &str) -> Result<(), crate::error::GddyError> {
    if std::path::Path::new(file).is_file() {
        return Ok(());
    }
    Err(crate::error::GddyError::validation(format!(
        "zip file not found: {file}"
    )))
}

/// Build the "poll status" next action, prefilling `--job-id` when the upload
/// response returned one, otherwise leaving it required for the user to fill in.
fn upload_status_next_action(app_id: String, upload_job_id: &str) -> cli_engine::NextAction {
    let job_id_param = if upload_job_id.is_empty() {
        NextActionParam::required()
    } else {
        NextActionParam::value(upload_job_id)
    };
    next_action(
        "hosting nodejs source status --app-id <app-id> --job-id <job-id>",
        "Poll zip upload status",
    )
    .with_param("app-id", NextActionParam::value(app_id))
    .with_param("job-id", job_id_param)
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
            require_zip_file(&file).map_err(crate::error::GddyError::into_cli_error)?;
            let path = std::path::Path::new(&file);
            let client = make_client(&ctx, &[CODE_WRITE]).await?;
            let data = client
                .upload_source(&app_id, path)
                .await
                .map_err(client_err)?;
            let upload_job_id = data.get("jobId").and_then(|v| v.as_str()).unwrap_or("");
            let action = upload_status_next_action(app_id, upload_job_id);
            Ok(CommandResult::new(data).with_next_actions(vec![action]))
        },
    )
}

#[cfg(test)]
mod tests {
    use cli_engine::NextActionParam;

    use super::{require_zip_file, upload_status_next_action};

    #[test]
    fn require_zip_file_rejects_missing_path() {
        let err = require_zip_file("/no/such/file.zip").expect_err("missing file should error");
        assert!(err.to_string().contains("zip file not found"));
        assert!(err.to_string().contains("/no/such/file.zip"));
    }

    #[test]
    fn require_zip_file_accepts_an_existing_file() {
        let tmp = tempfile::NamedTempFile::new().expect("failed to create temp file");
        let path = tmp.path().to_str().expect("temp path should be utf8");
        assert!(require_zip_file(path).is_ok());
    }

    #[test]
    fn require_zip_file_rejects_a_directory() {
        let tmp = tempfile::tempdir().expect("failed to create temp dir");
        let path = tmp.path().to_str().expect("temp path should be utf8");
        let err = require_zip_file(path).expect_err("directory should error");
        assert!(err.to_string().contains("zip file not found"));
    }

    #[test]
    fn upload_status_next_action_requires_job_id_when_absent() {
        let action = upload_status_next_action("app-123".to_owned(), "");
        assert_eq!(action.params["app-id"], NextActionParam::value("app-123"));
        assert_eq!(action.params["job-id"], NextActionParam::required());
    }

    #[test]
    fn upload_status_next_action_prefills_job_id_when_present() {
        let action = upload_status_next_action("app-123".to_owned(), "job-456");
        assert_eq!(action.params["job-id"], NextActionParam::value("job-456"));
    }
}
