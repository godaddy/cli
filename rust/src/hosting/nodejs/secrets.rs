//! `gddy hosting nodejs secrets` — manage app secrets.

use cli_engine::{
    CommandResult, CommandSpec, GroupSpec, RuntimeCommandSpec, RuntimeGroupSpec, Tier,
};
use serde_json::Value;

use crate::hosting::nodejs::{client_err, make_client};
use crate::scopes::HOSTING_SECRETS_WRITE as SECRETS_WRITE;

pub(super) fn group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(GroupSpec::new("secrets", "Manage app secrets"))
        .with_command(list_command())
        .with_command(update_command())
}

#[derive(Debug, Clone, clap::Args)]
struct SecretsListArgs {
    /// Application ID.
    #[arg(long = "app-id", value_name = "APP_ID")]
    app_id: String,

    /// Variant filter (preview or publish).
    #[arg(long, value_name = "VARIANT", value_parser = ["preview", "publish"])]
    variant: Option<String>,
}

fn list_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<SecretsListArgs, _, _, _>(
        CommandSpec::from_args::<SecretsListArgs>(
            "list",
            "List application secrets (metadata only)",
        )
        .with_long("List secret names and metadata. Secret values are never returned.")
        .with_system("hosting")
        .with_tier(Tier::Read)
        .with_scopes(&[SECRETS_WRITE]),
        |ctx, args: SecretsListArgs| async move {
            let app_id = args.app_id;
            let variant = args.variant;
            let client = make_client(&ctx, &[SECRETS_WRITE]).await?;
            let data = client
                .list_secrets(&app_id, variant.as_deref())
                .await
                .map_err(client_err)?;
            Ok(CommandResult::new(data))
        },
    )
}

#[derive(Debug, Clone, clap::Args)]
struct SecretsUpdateArgs {
    /// Application ID.
    #[arg(long = "app-id", value_name = "APP_ID")]
    app_id: String,

    /// JSON file with secret operations.
    #[arg(long, short = 'f', value_name = "PATH")]
    file: String,
}

fn update_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<SecretsUpdateArgs, _, _, _>(
        CommandSpec::from_args::<SecretsUpdateArgs>(
            "update",
            "Add, update, or delete application secrets",
        )
        .with_long(
            "Add, update, or delete secrets from a JSON file. The file must match the \
             `PublicSecretsUpdateBody` schema; use `secrets list` to see which secrets \
             currently exist.",
        )
        .with_system("hosting")
        .with_tier(Tier::Mutate)
        .mutates(true)
        .with_scopes(&[SECRETS_WRITE]),
        |ctx, args: SecretsUpdateArgs| async move {
            let app_id = args.app_id;
            let file = args.file;
            let content = std::fs::read_to_string(&file).map_err(|e| {
                crate::error::GddyError::validation(format!(
                    "failed to read secrets file '{file}': {e}"
                ))
                .into_cli_error()
            })?;
            let body: Value = serde_json::from_str(&content).map_err(|e| {
                crate::error::GddyError::validation(format!(
                    "invalid JSON in secrets file '{file}': {e}"
                ))
                .into_cli_error()
            })?;
            let client = make_client(&ctx, &[SECRETS_WRITE]).await?;
            let data = client
                .update_secrets(&app_id, body)
                .await
                .map_err(client_err)?;
            Ok(CommandResult::new(data))
        },
    )
}
