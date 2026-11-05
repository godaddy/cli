use cli_engine::{CommandResult, CommandSpec, NextActionParam, RuntimeCommandSpec, Tier};
use serde_json::json;

use crate::hosting::common::{AppIdArgs, client_err_retry_on_503, is_mhwp_app_id, make_client};
use crate::next_action::next_action;
use crate::scopes::HOSTING_APPLICATION_DELETE as APP_DELETE;

fn delete_long(mhwp: bool) -> String {
    let mut long =
        "Permanently delete a hosting application and all its associated data.".to_owned();
    if mhwp {
        long.push_str(
            "\n\nA WordPress (MHWP) site is deleted along with its content. The command \
             returns once deletion has started. `hosting app get` then shows status \
             DELETING until the site is gone, and after that reports it was not found. \
             There is no operation to poll.",
        );
    }
    long
}

pub(super) fn command(mhwp: bool) -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<AppIdArgs, _, _, _>(
        CommandSpec::from_args::<AppIdArgs>("delete", "Delete a hosting application")
            .with_long(delete_long(mhwp))
            .with_system("hosting")
            .with_tier(Tier::Destructive)
            .mutates(true)
            .with_scopes(&[APP_DELETE]),
        |ctx, args: AppIdArgs| async move {
            let app_id = args.app_id;
            let client = make_client(&ctx, &[APP_DELETE]).await?;
            client
                .delete_app(&app_id)
                .await
                .map_err(|e| client_err_retry_on_503(e, is_mhwp_app_id(&app_id)))?;
            Ok(delete_outcome(app_id))
        },
    )
}

/// A WordPress delete answers 204 once teardown has started. The site reads as
/// DELETING until a later get returns not found, and there is no operation id.
fn delete_outcome(app_id: String) -> CommandResult {
    if is_mhwp_app_id(&app_id) {
        return CommandResult::new(json!({ "status": "DELETING", "appId": &app_id }))
            .with_next_actions(vec![
                next_action(
                    "hosting app get --app-id <app-id>",
                    "Check whether this site is still being deleted",
                )
                .with_param("app-id", NextActionParam::value(app_id)),
            ]);
    }
    CommandResult::new(json!({ "deleted": true, "appId": app_id })).with_next_actions(vec![
        next_action(
            "hosting app list --app-type <type>",
            "List remaining applications",
        ),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hosting::test_support::{cli, run_hosting};

    #[test]
    fn wordpress_delete_reports_teardown_started() {
        for id in ["MHWP-a1b2c3d4e5", "mhwp-a1b2c3d4e5"] {
            let result = delete_outcome(id.to_owned());
            assert!(result.data.get("deleted").is_none(), "{id}");
            assert_eq!(result.data["status"], "DELETING");
            assert_eq!(result.data["appId"], id);
            let actions = &result.metadata.next_actions;
            assert_eq!(actions.len(), 1, "{id}");
            assert_eq!(actions[0].command, "gddy hosting app get --app-id <app-id>");
            assert_eq!(actions[0].params["app-id"].value.as_deref(), Some(id));
        }
    }

    #[test]
    fn nodejs_delete_stays_deleted_true() {
        let result = delete_outcome("NODEJS-aBcDeFgHiJ".to_owned());
        assert_eq!(result.data["deleted"], true);
        assert_eq!(result.data["appId"], "NODEJS-aBcDeFgHiJ");
        assert!(result.data.get("status").is_none());
        assert_eq!(
            result.metadata.next_actions[0].command,
            "gddy hosting app list --app-type <type>"
        );
    }

    #[tokio::test]
    async fn delete_help_says_wordpress_teardown_returns_before_the_site_is_gone() {
        let on = run_hosting(&cli(true), &["app", "delete", "--help"]).await;
        assert!(on.contains("deletion has started"), "{on}");
        assert!(on.contains("DELETING"), "{on}");
        assert!(on.contains("no operation to poll"), "{on}");

        let off = run_hosting(&cli(false), &["app", "delete", "--help"]).await;
        assert!(!off.contains("MHWP"), "{off}");
        assert!(!off.contains("WordPress"), "{off}");
    }
}
