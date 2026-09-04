use cli_engine::{CommandResult, CommandSpec, NextActionParam, RuntimeCommandSpec, Tier};

use crate::hosting::common::{client_err, make_client};
use crate::next_action::next_action;
use crate::scopes::HOSTING_SOURCE_WRITE as SOURCE_WRITE;

#[derive(Debug, Clone, clap::Args)]
struct SourceUploadArgs {
    /// Application ID.
    #[arg(long = "app-id", value_name = "APP_ID")]
    app_id: String,

    /// Path to the zip archive to upload.
    #[arg(long, short = 'f', value_name = "PATH")]
    file: String,
}

fn require_zip_file(file: &str) -> Result<(), crate::error::GddyError> {
    if std::path::Path::new(file).is_file() {
        return Ok(());
    }
    Err(crate::error::GddyError::validation(format!(
        "zip file not found: {file}"
    )))
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<SourceUploadArgs, _, _, _>(
        CommandSpec::from_args::<SourceUploadArgs>(
            "upload",
            "Upload a zip archive as the app's source code",
        )
        .with_long(
            "Upload a local zip archive as source code for an existing app. \
             Use `hosting source github` instead to import from a GitHub repository. \
             Returns immediately — poll `hosting source status` until status is \
             COMPLETED or FAILED. Then use `hosting deployment publish` to deploy.",
        )
        .with_system("hosting")
        .with_tier(Tier::Mutate)
        .mutates(true)
        .with_scopes(&[SOURCE_WRITE]),
        |ctx, args: SourceUploadArgs| async move {
            let app_id = args.app_id;
            let file = args.file;
            require_zip_file(&file).map_err(crate::error::GddyError::into_cli_error)?;
            let path = std::path::Path::new(&file);
            let client = make_client(&ctx, &[SOURCE_WRITE]).await?;
            let data = client
                .create_import_zip(&app_id, path)
                .await
                .map_err(client_err)?;
            let import_id = data
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_owned();
            let id_param = if import_id.is_empty() {
                NextActionParam::required()
            } else {
                NextActionParam::value(&import_id)
            };
            Ok(CommandResult::new(data).with_next_actions(vec![
                next_action(
                    "hosting source status --app-id <app-id> --import-id <id>",
                    "Poll until upload import completes",
                )
                .with_param("app-id", NextActionParam::value(app_id))
                .with_param("import-id", id_param),
            ]))
        },
    )
}

#[cfg(test)]
mod tests {
    use super::require_zip_file;

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
}
