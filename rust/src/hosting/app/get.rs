use cli_engine::{
    CommandResult, CommandSpec, NextAction, NextActionParam, RuntimeCommandSpec, Tier,
};

use crate::hosting::common::{
    AppIdArgs, HostingApplication, client_err, is_mhwp_app_id, make_client,
};
use crate::next_action::next_action;
use crate::scopes::HOSTING_APPLICATION_READ as APP_READ;

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<AppIdArgs, _, _, _>(
        CommandSpec::from_args::<AppIdArgs>("get", "Get a hosting application")
            .with_long(
                "Get details for a single hosting application by ID. \
                 The `urls` object exposes the reachable URLs for each environment \
                 variant (preview and publish).",
            )
            .with_system("hosting")
            .with_tier(Tier::Read)
            .with_scopes(&[APP_READ])
            .with_output_schema::<HostingApplication>(),
        |ctx, args: AppIdArgs| async move {
            let app_id = args.app_id;
            let client = make_client(&ctx, &[APP_READ]).await?;
            let data = client.get_app(&app_id).await.map_err(client_err)?;
            Ok(CommandResult::new(data).with_next_actions(next_actions(app_id)))
        },
    )
}

/// WordPress (MHWP) sites have no deployments, so they are not offered one.
///
/// Deliberately independent of the `hosting-mhwp` flag, unlike the retry hint
/// in `operation get`: this branch only withholds a suggestion that was wrong
/// for a WordPress id whatever the flag said, and the id reaches here because
/// the caller already has one.
fn next_actions(app_id: String) -> Vec<NextAction> {
    let status = next_action(
        "hosting app status --app-id <app-id>",
        "Get runtime status for this application",
    )
    .with_param("app-id", NextActionParam::value(app_id.clone()));
    if is_mhwp_app_id(&app_id) {
        return vec![status];
    }
    vec![
        status,
        next_action(
            "hosting deployment list --app-id <app-id>",
            "List deployments for this application",
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
    fn wordpress_sites_are_not_offered_deployments() {
        let mhwp = next_actions("MHWP-a1b2c3d4e5".to_owned());
        assert_eq!(
            commands(&mhwp),
            ["gddy hosting app status --app-id <app-id>"]
        );
        assert_eq!(
            mhwp[0].params["app-id"].value.as_deref(),
            Some("MHWP-a1b2c3d4e5")
        );
    }

    #[test]
    fn nodejs_apps_keep_status_and_deployments() {
        assert_eq!(
            commands(&next_actions("NODEJS-a1b2c3d4e5".to_owned())),
            [
                "gddy hosting app status --app-id <app-id>",
                "gddy hosting deployment list --app-id <app-id>",
            ]
        );
    }
}
