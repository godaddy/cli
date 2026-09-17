use cli_engine::{CommandResult, CommandSpec, NextActionParam, RuntimeCommandSpec, Tier};
use serde_json::json;

use crate::hosting::common::{client_err, make_client};
use crate::next_action::next_action;
use crate::scopes::HOSTING_SECRET_WRITE as SECRET_WRITE;

#[derive(Debug, Clone, clap::Args)]
struct SecretDeleteArgs {
    /// Application ID.
    #[arg(long = "app-id", value_name = "APP_ID")]
    app_id: String,

    /// Secret name.
    #[arg(long, value_name = "NAME")]
    name: String,

    /// Target environment (PREVIEW or PUBLISH). Defaults to PREVIEW.
    #[arg(long, value_name = "VARIANT", value_parser = ["PREVIEW", "PUBLISH"], default_value = "PREVIEW")]
    variant: String,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<SecretDeleteArgs, _, _, _>(
        CommandSpec::from_args::<SecretDeleteArgs>("delete", "Delete an application secret")
            .with_long(
                "Delete a secret from a hosting application. \
                 Use --variant to target PREVIEW or PUBLISH (defaults to PREVIEW). \
                 System-managed secrets cannot be deleted.",
            )
            .with_system("hosting")
            .with_tier(Tier::Destructive)
            .mutates(true)
            .with_scopes(&[SECRET_WRITE]),
        |ctx, args: SecretDeleteArgs| async move {
            let app_id = args.app_id.clone();
            let name = args.name.clone();
            let body = json!({
                "variant": args.variant,
                "operations": {
                    "deletions": [{ "name": args.name }]
                }
            });
            let client = make_client(&ctx, &[SECRET_WRITE]).await?;
            let data = client
                .sync_secrets(&app_id, body)
                .await
                .map_err(client_err)?;
            Ok(CommandResult::new(data).with_next_actions(vec![
                next_action(
                    "hosting secrets list --app-id <app-id>",
                    "Verify the secret was removed",
                )
                .with_param("app-id", NextActionParam::value(app_id))
                .with_param("name", NextActionParam::value(name)),
            ]))
        },
    )
}
