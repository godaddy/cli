use cli_engine::{CommandSpec, RuntimeCommandSpec, Tier};

use crate::hosting::app::common::{PublishAppIdArgs, set_publish_state};
use crate::hosting::common::HostingAppOperation;
use crate::scopes::HOSTING_APPLICATION_UPDATE as APP_UPDATE;

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<PublishAppIdArgs, _, _, _>(
        CommandSpec::from_args::<PublishAppIdArgs>("unpublish", "Take a WordPress site offline")
            .with_long(
                "Take an MHWP (WordPress) application back behind its Coming Soon page. The site \
                 and its content stay in place; only visitors stop seeing it. Returns \
                 immediately with an operation ID; poll `hosting operation get` until status is \
                 COMPLETED or FAILED. A site that is already offline is accepted and nothing \
                 changes. Use `hosting app publish` to take it live again.",
            )
            .with_system("hosting")
            .with_tier(Tier::Mutate)
            .mutates(true)
            .with_scopes(&[APP_UPDATE])
            .with_output_schema::<HostingAppOperation>(),
        |ctx, args: PublishAppIdArgs| async move { set_publish_state(&ctx, args.app_id, false).await },
    )
}
