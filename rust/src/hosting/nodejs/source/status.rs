use cli_engine::{CommandResult, CommandSpec, NextActionParam, RuntimeCommandSpec, Tier};

use super::AppJobIdArgs;
use crate::hosting::nodejs::{client_err, is_source_job_terminal, make_client};
use crate::next_action::next_action;
use crate::scopes::HOSTING_CODE_WRITE as CODE_WRITE;

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<AppJobIdArgs, _, _, _>(
        CommandSpec::from_args::<AppJobIdArgs>("status", "Poll zip upload status")
            .with_long(
                "Poll a zip source upload job until status is `complete` or `failed`. \
                 When `complete`, the source has been deployed to the app's preview \
                 environment; run `deployment publish` to promote to live.",
            )
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
