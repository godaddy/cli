use cli_engine::{
    CommandResult, CommandSpec, NextAction, NextActionParam, RuntimeCommandSpec, Tier,
};

use crate::hosting::common::{
    AppIdArgs, HostingApplicationStatus, client_err, is_mhwp_app_id, make_client,
};
use crate::next_action::next_action;
use crate::scopes::HOSTING_APPLICATION_READ as APP_READ;

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<AppIdArgs, _, _, _>(
        CommandSpec::from_args::<AppIdArgs>("status", "Get application runtime status")
            .with_long(
                "Get the runtime status of a hosting application's environments \
                 (preview and publish). Use this to check whether an environment \
                 is ACTIVE, IDLE, or in a transitional state after a restart or deployment. \
                 The `variants` array contains one entry per environment (PREVIEW and PUBLISH) \
                 with the runtime status for that environment.",
            )
            .with_system("hosting")
            .with_tier(Tier::Read)
            .with_scopes(&[APP_READ])
            .with_output_schema::<HostingApplicationStatus>(),
        |ctx, args: AppIdArgs| async move {
            let app_id = args.app_id;
            let client = make_client(&ctx, &[APP_READ]).await?;
            let data = client.get_app_status(&app_id).await.map_err(client_err)?;
            Ok(CommandResult::new(data).with_next_actions(next_actions(app_id)))
        },
    )
}

/// WordPress (MHWP) sites have no deployments and no restart, so they are
/// pointed back at the site's details instead.
///
/// Deliberately independent of the `hosting-mhwp` flag, unlike the retry hint
/// in `operation get`: this branch only withholds suggestions that were wrong
/// for a WordPress id whatever the flag said, and `app get` is reachable with
/// the flag off.
fn next_actions(app_id: String) -> Vec<NextAction> {
    if is_mhwp_app_id(&app_id) {
        return vec![
            next_action(
                "hosting app get --app-id <app-id>",
                "Get details and the live URL for this site",
            )
            .with_param("app-id", NextActionParam::value(app_id)),
        ];
    }
    vec![
        next_action(
            "hosting deployment list --app-id <app-id>",
            "List deployments for this application",
        )
        .with_param("app-id", NextActionParam::value(app_id.clone())),
        next_action(
            "hosting app restart --app-id <app-id> --variant <variant>",
            "Restart an environment",
        )
        .with_param("app-id", NextActionParam::value(app_id)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commands(actions: &[NextAction]) -> Vec<&str> {
        actions.iter().map(|a| a.command.as_str()).collect()
    }

    #[test]
    fn wordpress_sites_are_not_offered_deployments_or_restart() {
        // The edge matches the prefix case-insensitively; so does the hint.
        for id in ["MHWP-a1b2c3d4e5", "mhwp-a1b2c3d4e5"] {
            let mhwp = next_actions(id.to_owned());
            assert_eq!(
                commands(&mhwp),
                ["gddy hosting app get --app-id <app-id>"],
                "{id}"
            );
            assert_eq!(mhwp[0].params["app-id"].value.as_deref(), Some(id));
        }
    }

    #[test]
    fn nodejs_apps_keep_deployments_and_restart() {
        assert_eq!(
            commands(&next_actions("NODEJS-a1b2c3d4e5".to_owned())),
            [
                "gddy hosting deployment list --app-id <app-id>",
                "gddy hosting app restart --app-id <app-id> --variant <variant>",
            ]
        );
    }
}
