//! `gddy hosting nodejs job` — poll app creation jobs.

use cli_engine::{
    CommandResult, CommandSpec, GroupSpec, NextActionParam, RuntimeCommandSpec, RuntimeGroupSpec,
    Tier,
};
use serde_json::Value;

use crate::hosting::nodejs::{
    HostingJobSummary, client_err, is_app_creation_job_terminal, make_client,
};
use crate::next_action::next_action;
use crate::scopes::HOSTING_APPS_CREATE as APPS_CREATE;

pub(super) fn group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(GroupSpec::new("job", "Poll app creation jobs")).with_command(command())
}

#[derive(Debug, Clone, clap::Args)]
struct JobIdArgs {
    /// App creation job ID.
    #[arg(long = "job-id", value_name = "JOB_ID")]
    job_id: String,
}

fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<JobIdArgs, _, _, _>(
        CommandSpec::from_args::<JobIdArgs>("get", "Get app creation job status")
            .with_long(
                "Poll an app creation job until status is `active` (app provisioned) or \
                 `failed`. Only used after `app create` — for source upload or git import \
                 jobs use `source status` or `source git-status`.",
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

#[cfg(test)]
mod tests {
    use super::promote_job_fields;
    use serde_json::json;

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
