use cli_engine::{CommandResult, CommandSpec, RuntimeCommandSpec, Tier};
use serde_json::{Value, json};

use crate::hosting::nodejs::{client_err, make_client};
use crate::scopes::HOSTING_APPS_UPDATE as APPS_UPDATE;

#[derive(Debug, Clone, clap::Args)]
struct AppUpdateArgs {
    /// Application ID.
    #[arg(long = "app-id", value_name = "APP_ID")]
    app_id: String,

    /// New application name.
    #[arg(long, value_name = "NAME")]
    name: Option<String>,

    /// Application root path.
    #[arg(long = "root-path", value_name = "PATH")]
    root_path: Option<String>,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<AppUpdateArgs, _, _, _>(
        CommandSpec::from_args::<AppUpdateArgs>(
            "update",
            "Update Node.js hosting application metadata",
        )
        .with_long("Update app metadata (name and/or root path). Does not affect source code or deployments.")
        .with_system("hosting")
        .with_tier(Tier::Mutate)
        .mutates(true)
        .with_scopes(&[APPS_UPDATE]),
        |ctx, args: AppUpdateArgs| async move {
            let app_id = args.app_id;
            // Treat an empty/whitespace-only value the same as an absent flag,
            // matching the pre-typed-args `optional_str` helper's behavior —
            // otherwise `--name ""` bypasses the "at least one of" check below
            // and would send an invalid patch body.
            let name = args.name.filter(|s| !s.trim().is_empty());
            let root_path = args.root_path.filter(|s| !s.trim().is_empty());
            if name.is_none() && root_path.is_none() {
                return Err(crate::error::GddyError::validation(
                    "at least one of --name or --root-path is required",
                )
                .into_cli_error());
            }
            let mut body = serde_json::Map::new();
            if let Some(name) = name {
                body.insert("name".to_owned(), json!(name));
            }
            if let Some(root_path) = root_path {
                body.insert("rootPath".to_owned(), json!(root_path));
            }
            let client = make_client(&ctx, &[APPS_UPDATE]).await?;
            let data = client
                .patch_app(&app_id, Value::Object(body))
                .await
                .map_err(client_err)?;
            Ok(CommandResult::new(data))
        },
    )
}
