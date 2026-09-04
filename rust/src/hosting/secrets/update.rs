use cli_engine::{CommandResult, CommandSpec, NextActionParam, RuntimeCommandSpec, Tier};
use serde_json::json;

use crate::hosting::common::{client_err, make_client};
use crate::next_action::next_action;
use crate::scopes::HOSTING_SECRET_WRITE as SECRET_WRITE;

#[derive(Debug, Clone, clap::Args)]
struct SecretUpdateArgs {
    /// Application ID.
    #[arg(long = "app-id", value_name = "APP_ID")]
    app_id: String,

    /// Secret name.
    #[arg(long, value_name = "NAME")]
    name: String,

    /// New secret value.
    #[arg(long, value_name = "VALUE")]
    value: String,

    /// Target environment (PREVIEW or PUBLISH). Defaults to PREVIEW.
    #[arg(long, value_name = "VARIANT", value_parser = ["PREVIEW", "PUBLISH"], default_value = "PREVIEW")]
    variant: String,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<SecretUpdateArgs, _, _, _>(
        CommandSpec::from_args::<SecretUpdateArgs>("update", "Update an application secret")
            .with_long(
                "Update the value of an existing secret for a hosting application. \
                 Use --variant to target PREVIEW or PUBLISH (defaults to PREVIEW).",
            )
            .with_system("hosting")
            .with_tier(Tier::Mutate)
            .mutates(true)
            .with_scopes(&[SECRET_WRITE]),
        |ctx, args: SecretUpdateArgs| async move {
            let app_id = args.app_id.clone();
            let body = json!({
                "variant": args.variant,
                "operations": {
                    "updates": [{ "name": args.name, "value": args.value }]
                }
            });
            let client = make_client(&ctx, &[SECRET_WRITE]).await?;
            let data = client
                .sync_secrets(&app_id, body)
                .await
                .map_err(client_err)?;
            Ok(CommandResult::new(data).with_next_actions(vec![
                next_action("hosting secrets list --app-id <app-id>", "View all secrets")
                    .with_param("app-id", NextActionParam::value(app_id)),
            ]))
        },
    )
}
