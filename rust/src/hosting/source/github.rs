use cli_engine::{CommandResult, CommandSpec, NextActionParam, RuntimeCommandSpec, Tier};

use crate::hosting::common::{HostingSourceImport, client_err, make_client};
use crate::next_action::next_action;
use crate::scopes::HOSTING_SOURCE_WRITE as SOURCE_WRITE;

#[derive(Debug, Clone, clap::Args)]
struct SourceGithubArgs {
    /// Application ID.
    #[arg(long = "app-id", value_name = "APP_ID")]
    app_id: String,

    /// GitHub repository in owner/repo format.
    #[arg(long, value_name = "OWNER/REPO")]
    repo: String,

    /// Branch to import from.
    #[arg(long, value_name = "BRANCH")]
    branch: String,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<SourceGithubArgs, _, _, _>(
        CommandSpec::from_args::<SourceGithubArgs>(
            "github",
            "Import source from a GitHub repository",
        )
        .with_long(
            "Start a source import from a GitHub repository branch. \
                 Use `hosting source upload` instead when you have a local zip archive. \
                 Returns immediately — poll `hosting source status` until status is \
                 COMPLETED or FAILED. Then use `hosting deployment publish` to deploy.",
        )
        .with_system("hosting")
        .with_tier(Tier::Mutate)
        .mutates(true)
        .with_scopes(&[SOURCE_WRITE])
        .with_output_schema::<HostingSourceImport>(),
        |ctx, args: SourceGithubArgs| async move {
            let app_id = args.app_id.clone();
            let client = make_client(&ctx, &[SOURCE_WRITE]).await?;
            let data = client
                .create_import(&app_id, &args.repo, &args.branch)
                .await
                .map_err(client_err)?;
            let import_id = data
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_owned();
            Ok(CommandResult::new(data).with_next_actions(vec![
                next_action(
                    "hosting source status --app-id <app-id> --import-id <id>",
                    "Poll until import completes",
                )
                .with_param("app-id", NextActionParam::value(app_id))
                .with_param("import-id", NextActionParam::value(import_id)),
            ]))
        },
    )
}
