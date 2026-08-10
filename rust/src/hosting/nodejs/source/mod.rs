//! `gddy hosting nodejs source` — push code to an app's preview environment.

mod git;
mod status;
mod upload;

use cli_engine::{GroupSpec, RuntimeGroupSpec};

/// Application ID + job ID, shared by the zip-upload and git-import status polls.
#[derive(Debug, Clone, clap::Args)]
struct AppJobIdArgs {
    /// Application ID.
    #[arg(long = "app-id", value_name = "APP_ID")]
    app_id: String,

    /// Job ID.
    #[arg(long = "job-id", value_name = "JOB_ID")]
    job_id: String,
}

pub(super) fn group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(GroupSpec::new(
        "source",
        "Push code to an app's preview environment",
    ))
    .with_command(upload::command())
    .with_command(status::command())
    .with_command(git::command())
    .with_command(git::status_command())
}
