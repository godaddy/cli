use cli_engine::{
    CommandResult, CommandSpec, NextAction, NextActionParam, RuntimeCommandSpec, Tier,
};
use serde_json::Value;

use crate::hosting::common::{HostingSourceImport, client_err, make_client};
use crate::next_action::next_action;
use crate::scopes::HOSTING_SOURCE_READ as SOURCE_READ;

#[derive(Debug, Clone, clap::Args)]
struct SourceStatusArgs {
    /// Application ID.
    #[arg(long = "app-id", value_name = "APP_ID")]
    app_id: String,

    /// Import ID returned by `hosting source upload` or `hosting source github`.
    #[arg(long = "import-id", value_name = "IMPORT_ID")]
    import_id: String,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<SourceStatusArgs, _, _, _>(
        CommandSpec::from_args::<SourceStatusArgs>("status", "Get the status of a source import")
            .with_long(
                "Poll the status of a source import started with `hosting source upload` or \
                 `hosting source github`. Keep polling until status is COMPLETED or FAILED. \
                 Once COMPLETED, check that the app has a hosting plan with \
                 `hosting subscription get`, then publish with `hosting deployment publish`.",
            )
            .with_system("hosting")
            .with_tier(Tier::Read)
            .with_scopes(&[SOURCE_READ])
            .with_output_schema::<HostingSourceImport>(),
        |ctx, args: SourceStatusArgs| async move {
            let app_id = args.app_id.clone();
            let client = make_client(&ctx, &[SOURCE_READ]).await?;
            let data = client
                .get_import(&app_id, &args.import_id)
                .await
                .map_err(client_err)?;
            let next_actions = next_actions(&data, &app_id);
            Ok(CommandResult::new(data).with_next_actions(next_actions))
        },
    )
}

fn next_actions(data: &Value, app_id: &str) -> Vec<NextAction> {
    if data["status"] != "COMPLETED" {
        return Vec::new();
    }
    vec![
        next_action(
            "hosting subscription get --app-id <app-id>",
            "Check the app has a hosting plan before publishing",
        )
        .with_param("app-id", NextActionParam::value(app_id)),
        next_action(
            "hosting deployment publish --app-id <app-id>",
            "Publish to production. The app needs a hosting plan",
        )
        .with_param("app-id", NextActionParam::value(app_id)),
    ]
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn completed_import_suggests_plan_check_then_publish() {
        let actions = next_actions(&json!({ "status": "COMPLETED" }), "NODEJS-1");
        let commands: Vec<&str> = actions.iter().map(|a| a.command.as_str()).collect();
        assert_eq!(
            commands,
            [
                "gddy hosting subscription get --app-id <app-id>",
                "gddy hosting deployment publish --app-id <app-id>",
            ]
        );
        assert!(
            actions
                .iter()
                .all(|a| a.params["app-id"].value.as_deref() == Some("NODEJS-1"))
        );
    }

    #[test]
    fn unfinished_or_failed_import_suggests_nothing() {
        for status in ["IN_PROGRESS", "FAILED"] {
            assert!(next_actions(&json!({ "status": status }), "NODEJS-1").is_empty());
        }
        assert!(next_actions(&json!({}), "NODEJS-1").is_empty());
    }
}
