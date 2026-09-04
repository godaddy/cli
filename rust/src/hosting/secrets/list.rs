use cli_engine::{CommandResult, CommandSpec, NextActionParam, RuntimeCommandSpec, Tier};

use crate::hosting::common::{HostingSecretSummary, client_err, make_client};
use crate::next_action::next_action;
use crate::scopes::HOSTING_SECRET_READ as SECRET_READ;

#[derive(Debug, Clone, clap::Args)]
struct SecretsListArgs {
    /// Application ID.
    #[arg(long = "app-id", value_name = "APP_ID")]
    app_id: String,

    /// Filter by environment variant (PREVIEW or PUBLISH).
    #[arg(long, value_name = "VARIANT", value_parser = ["PREVIEW", "PUBLISH"])]
    variant: Option<String>,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<SecretsListArgs, _, _, _>(
        CommandSpec::from_args::<SecretsListArgs>("list", "List application secrets")
            .with_long(
                "List secret names for a hosting application. Values are never returned. \
                 Use --variant to filter by environment; omit to see all secrets.",
            )
            .with_system("hosting")
            .with_tier(Tier::Read)
            .with_scopes(&[SECRET_READ])
            .with_default_fields("name,scope")
            .with_output_schema::<HostingSecretSummary>(),
        |ctx, args: SecretsListArgs| async move {
            let app_id = args.app_id.clone();
            let client = make_client(&ctx, &[SECRET_READ]).await?;
            let data = client
                .list_secrets(&app_id, args.variant.as_deref())
                .await
                .map_err(client_err)?;
            Ok(CommandResult::new(data).with_next_actions(vec![
                next_action(
                    "hosting secrets create --app-id <app-id> --name <name> --value <value>",
                    "Add a new secret",
                )
                .with_param("app-id", NextActionParam::value(app_id)),
            ]))
        },
    )
}
