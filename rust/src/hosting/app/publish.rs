use cli_engine::{CommandSpec, RuntimeCommandSpec, Tier};

use crate::hosting::app::common::{PublishAppIdArgs, set_publish_state};
use crate::hosting::common::HostingAppOperation;
use crate::scopes::HOSTING_APPLICATION_UPDATE as APP_UPDATE;

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<PublishAppIdArgs, _, _, _>(
        CommandSpec::from_args::<PublishAppIdArgs>("publish", "Take a WordPress site live")
            .with_long(
                "Take an MHWP (WordPress) application live at its publish URL. WordPress sites \
                 have no build step: the site is provisioned ready to serve and starts as \
                 Coming Soon, and this switches it on. Returns immediately with an operation ID; \
                 poll `hosting operation get` until status is COMPLETED or FAILED. A site that is \
                 already live is accepted and nothing changes. Going live needs a plan with a \
                 free published-site slot; the free plan has none. Use `hosting app unpublish` \
                 to take the site back to Coming Soon. Node.js apps go live with \
                 `hosting deployment publish` instead.",
            )
            .with_system("hosting")
            .with_tier(Tier::Mutate)
            .mutates(true)
            .with_scopes(&[APP_UPDATE])
            .with_output_schema::<HostingAppOperation>(),
        |ctx, args: PublishAppIdArgs| async move { set_publish_state(&ctx, args.app_id, true).await },
    )
}

#[cfg(test)]
mod tests {
    use crate::hosting::test_support::{cli, run_hosting};

    #[tokio::test]
    async fn publish_and_unpublish_exist_only_while_mhwp_is_on() {
        let off = cli(false);
        let help = run_hosting(&off, &["app", "--help"]).await;
        assert!(!help.contains("publish"), "{help}");
        let missing = run_hosting(&off, &["app", "publish", "--app-id", "MHWP-a1b2c3d4e5"]).await;
        assert!(missing.contains("unknown command \"publish\""), "{missing}");

        let on = cli(true);
        let help = run_hosting(&on, &["app", "--help"]).await;
        assert!(
            help.contains("publish") && help.contains("unpublish"),
            "{help}"
        );
        let help = run_hosting(&on, &["app", "publish", "--help"]).await;
        assert!(help.contains("Coming Soon"), "{help}");
        assert!(help.contains("--app-id"), "{help}");
        let help = run_hosting(&on, &["app", "unpublish", "--help"]).await;
        assert!(help.contains("Coming Soon"), "{help}");
        // Parses, then stops at auth: no provider is registered in tests.
        let parsed = run_hosting(&on, &["app", "publish", "--app-id", "MHWP-a1b2c3d4e5"]).await;
        assert!(parsed.contains("no provider registered"), "{parsed}");
    }

    #[tokio::test]
    async fn help_text_names_publish_only_while_mhwp_is_on() {
        // `hosting deployment publish` is a Node.js command and stays in every mode, so
        // the check is for the WordPress commands themselves.
        let screens: [&[&str]; 3] = [
            &["--help"],
            &["operation", "--help"],
            &["operation", "get", "--help"],
        ];
        let off = cli(false);
        let on = cli(true);
        for args in screens {
            let help = run_hosting(&off, args).await;
            assert!(
                !help.contains("unpublish") && !help.contains("app publish"),
                "{args:?}: {help}"
            );
            let help = run_hosting(&on, args).await;
            assert!(
                help.contains("unpublish") || help.contains("publish/unpublish"),
                "{args:?}: {help}"
            );
        }
    }
}
