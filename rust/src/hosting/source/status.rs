use cli_engine::{CommandResult, CommandSpec, NextActionParam, RuntimeCommandSpec, Tier};

use crate::hosting::common::{client_err, make_client};
use crate::next_action::next_action;
use crate::scopes::HOSTING_SOURCE_READ as SOURCE_READ;

#[derive(Debug, Clone, clap::Args)]
struct SourceStatusArgs {
    /// Application ID.
    #[arg(long = "app-id", value_name = "APP_ID")]
    app_id: String,

    /// Import ID returned by `hosting source import`.
    #[arg(long = "import-id", value_name = "IMPORT_ID")]
    import_id: String,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<SourceStatusArgs, _, _, _>(
        CommandSpec::from_args::<SourceStatusArgs>("status", "Get the status of a source import")
            .with_long(
                "Poll the status of a source import started with `hosting source import`. \
                 Keep polling until status is COMPLETED or FAILED, \
                 then use `hosting deployment publish` to deploy.",
            )
            .with_system("hosting")
            .with_tier(Tier::Read)
            .with_scopes(&[SOURCE_READ]),
        |ctx, args: SourceStatusArgs| async move {
            let app_id = args.app_id.clone();
            let client = make_client(&ctx, &[SOURCE_READ]).await?;
            let data = client
                .get_import(&app_id, &args.import_id)
                .await
                .map_err(client_err)?;
            Ok(CommandResult::new(data).with_next_actions(vec![
                next_action(
                    "hosting deployment publish --app-id <app-id>",
                    "Deploy the imported source",
                )
                .with_param("app-id", NextActionParam::value(app_id)),
            ]))
        },
    )
}
