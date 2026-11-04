use cli_engine::{CommandSpec, RuntimeCommandSpec, Tier};

use crate::hosting::app::common::{UnpublishArgs, set_publish_state};
use crate::hosting::common::{HostingAppOperation, make_client};
use crate::scopes::HOSTING_DEPLOYMENT_EXECUTE as DEPLOY_EXECUTE;

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<UnpublishArgs, _, _, _>(
        CommandSpec::from_args::<UnpublishArgs>("unpublish", "Take a WordPress site offline")
            .with_long(
                "Take an MHWP (WordPress) application back behind a placeholder page. The site \
                 and its content stay in place; only visitors stop seeing it. Returns \
                 immediately with an operation ID; poll `hosting operation get` until status is \
                 COMPLETED or FAILED. A site that is already offline is accepted and nothing \
                 changes. Use `hosting app publish` to take it live again. Node.js apps have no \
                 placeholder page, so they cannot be unpublished.",
            )
            .with_system("hosting")
            .with_tier(Tier::Mutate)
            .mutates(true)
            .with_scopes(&[DEPLOY_EXECUTE])
            .with_output_schema::<HostingAppOperation>(),
        |ctx, args: UnpublishArgs| async move {
            let client = make_client(&ctx, &[DEPLOY_EXECUTE]).await?;
            set_publish_state(&client, args.app_id, false).await
        },
    )
}
