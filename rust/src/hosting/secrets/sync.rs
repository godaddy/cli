use cli_engine::{CommandResult, CommandSpec, NextActionParam, RuntimeCommandSpec, Tier};
use serde_json::{Value, json};

use crate::hosting::common::{HostingSecretSummary, client_err, make_client};
use crate::next_action::next_action;
use crate::scopes::HOSTING_SECRET_WRITE as SECRET_WRITE;

#[derive(Debug, Clone, clap::Args)]
struct SecretSyncArgs {
    /// Application ID.
    #[arg(long = "app-id", value_name = "APP_ID")]
    app_id: String,

    /// Target environment (PREVIEW or PUBLISH). Defaults to PREVIEW.
    #[arg(long, value_name = "VARIANT", value_parser = ["PREVIEW", "PUBLISH"], default_value = "PREVIEW")]
    variant: String,

    /// Secrets to add, as a JSON array: '[{"name":"K","value":"V"}]'.
    #[arg(long, value_name = "JSON")]
    additions: Option<String>,

    /// Secrets to update, as a JSON array: '[{"name":"K","value":"V"}]'.
    #[arg(long, value_name = "JSON")]
    updates: Option<String>,

    /// Secrets to delete, as a JSON array: '[{"name":"K"}]'.
    #[arg(long, value_name = "JSON")]
    deletions: Option<String>,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<SecretSyncArgs, _, _, _>(
        CommandSpec::from_args::<SecretSyncArgs>("sync", "Sync secrets in bulk")
            .with_long(
                "Apply many secret changes in a single call. Prefer this over separate \
                 `create`/`update`/`delete` invocations when changing several secrets at once — \
                 it is faster and applies as one batch. Accepts additions, updates, and \
                 deletions together; at least one of --additions, --updates, or --deletions \
                 is required. Each value is a JSON array.",
            )
            .with_system("hosting")
            .with_tier(Tier::Mutate)
            .mutates(true)
            .with_scopes(&[SECRET_WRITE])
            .with_output_schema::<HostingSecretSummary>(),
        |ctx, args: SecretSyncArgs| async move {
            let app_id = args.app_id.clone();

            let parse_json_array =
                |s: Option<String>, flag: &str| -> cli_engine::Result<Option<Value>> {
                    match s {
                        None => Ok(None),
                        Some(raw) => serde_json::from_str::<Value>(&raw).map(Some).map_err(|e| {
                            crate::error::GddyError::validation(format!(
                                "--{flag} is not valid JSON: {e}"
                            ))
                            .into_cli_error()
                        }),
                    }
                };

            let additions = parse_json_array(args.additions, "additions")?;
            let updates = parse_json_array(args.updates, "updates")?;
            let deletions = parse_json_array(args.deletions, "deletions")?;

            if additions.is_none() && updates.is_none() && deletions.is_none() {
                return Err(crate::error::GddyError::validation(
                    "at least one of --additions, --updates, or --deletions is required",
                )
                .into_cli_error());
            }

            let mut operations = serde_json::Map::new();
            if let Some(v) = additions {
                operations.insert("additions".to_owned(), v);
            }
            if let Some(v) = updates {
                operations.insert("updates".to_owned(), v);
            }
            if let Some(v) = deletions {
                operations.insert("deletions".to_owned(), v);
            }

            let body = json!({
                "variant": args.variant,
                "operations": Value::Object(operations),
            });

            let client = make_client(&ctx, &[SECRET_WRITE]).await?;
            let data = client
                .sync_secrets(&app_id, body)
                .await
                .map_err(client_err)?;
            Ok(CommandResult::new(data).with_next_actions(vec![
                next_action(
                    "hosting secrets list --app-id <app-id>",
                    "View updated secrets",
                )
                .with_param("app-id", NextActionParam::value(app_id)),
            ]))
        },
    )
}
