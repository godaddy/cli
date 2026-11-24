mod attach;
mod get;
mod list;

use cli_engine::{CliCoreError, GroupSpec, RuntimeGroupSpec};

use crate::hosting::client::ClientError;
use crate::hosting::common::{client_err, client_err_with_fix, is_mhwp_app_id};

const NO_PLAN_TO_ATTACH_NODEJS: &str = "This app is on Airo App Builder and is billed by your Airo \
     App Builder plan, so it has no hosting plan to show or attach. To take it live, run: \
     gddy hosting app publish --app-id <app-id>";
const NO_PLAN_TO_ATTACH_WORDPRESS: &str = "A WordPress site is billed by your Airo App Builder \
     subscription, so it has no hosting plan to show or attach. To take it live, run: \
     gddy hosting app publish --app-id <app-id>";

/// The error of `subscription get` and `subscription attach`. A `501` there
/// is not a fault: the app is billed by an Airo App Builder plan and takes no
/// hosting plan, so the fix says that and names the step that does apply.
fn attachment_err(e: ClientError, app_id: &str) -> CliCoreError {
    if !matches!(e, ClientError::Http { status: 501, .. }) {
        return client_err(e);
    }
    let fix = if is_mhwp_app_id(app_id) {
        NO_PLAN_TO_ATTACH_WORDPRESS
    } else {
        NO_PLAN_TO_ATTACH_NODEJS
    };
    client_err_with_fix(e, fix)
}

pub(super) fn group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(
        GroupSpec::new("subscription", "Manage hosting plan subscriptions")
            .with_alias("subscriptions")
            .with_long(
                "List available hosting plan subscriptions and attach one to an application. \
             A subscription represents a hosting plan (resources, billing) that backs \
             the application's runtime. Only a Node.js app on a Web Hosting plan takes \
             one. A Node.js app on Airo App Builder (`hostingProduct` AIRO in `hosting \
             app get`) and a WordPress site are billed by your Airo App Builder plan and \
             have no plan to attach.",
            ),
    )
    .with_command(list::command())
    .with_command(get::command())
    .with_command(attach::command())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fix_for(status: u16, app_id: &str) -> Option<String> {
        let err = attachment_err(
            ClientError::Http {
                status,
                body: r#"{"message":"m","details":[{"issue":"NOT_APPLICABLE"}]}"#.to_owned(),
            },
            app_id,
        );
        cli_engine::build_error_envelope(&err, "hosting").fix
    }

    #[test]
    fn a_501_says_the_app_has_no_plan_to_attach() {
        for (app_id, expected) in [
            ("NODEJS-aBcDeFgHiJ", "This app is on Airo App Builder"),
            ("nodejs-aBcDeFgHiJ", "This app is on Airo App Builder"),
            ("MHWP-a1b2c3d4e5", "A WordPress site is billed"),
        ] {
            let fix = fix_for(501, app_id);
            assert!(
                fix.as_deref().is_some_and(|f| f.contains(expected)
                    && f.contains("no hosting plan to show or attach")
                    && f.contains("gddy hosting app publish --app-id <app-id>")),
                "{app_id}: {fix:?}"
            );
        }
    }

    #[test]
    fn other_errors_keep_their_fix() {
        let fix = fix_for(404, "NODEJS-aBcDeFgHiJ");
        assert_eq!(fix.as_deref(), Some("Use: gddy hosting app list"));
        let network = attachment_err(ClientError::Network("down".to_owned()), "NODEJS-x");
        let fix = cli_engine::build_error_envelope(&network, "hosting").fix;
        assert!(
            fix.as_deref().is_some_and(|f| !f.contains("Airo")),
            "{fix:?}"
        );
    }
}
