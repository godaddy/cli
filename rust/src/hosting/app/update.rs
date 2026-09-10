use cli_engine::{CommandResult, CommandSpec, RuntimeCommandSpec, Tier};
use serde_json::{Value, json};

use crate::hosting::common::{HostingApplication, client_err, make_client};
use crate::scopes::HOSTING_APPLICATION_UPDATE as APP_UPDATE;

#[derive(Debug, Clone, clap::Args)]
struct AppUpdateArgs {
    /// Application ID.
    #[arg(long = "app-id", value_name = "APP_ID")]
    app_id: String,

    /// New display name (1–200 characters).
    #[arg(long, value_name = "NAME")]
    name: Option<String>,

    /// New root path.
    #[arg(long = "root-path", value_name = "PATH")]
    root_path: Option<String>,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<AppUpdateArgs, _, _, _>(
        CommandSpec::from_args::<AppUpdateArgs>("update", "Update a hosting application")
            .with_long(
                "Update application metadata. At least one of --name or --root-path \
                 is required. Changes take effect immediately and do not trigger a \
                 new deployment.",
            )
            .with_system("hosting")
            .with_tier(Tier::Mutate)
            .mutates(true)
            .with_scopes(&[APP_UPDATE])
            .with_output_schema::<HostingApplication>(),
        |ctx, args: AppUpdateArgs| async move {
            let app_id = args.app_id;
            let name = args.name.filter(|s| !s.trim().is_empty());
            let root_path = args.root_path.filter(|s| !s.trim().is_empty());

            if name.is_none() && root_path.is_none() {
                return Err(crate::error::GddyError::validation(
                    "at least one of --name or --root-path is required",
                )
                .into_cli_error());
            }

            let mut ops: Vec<Value> = Vec::new();
            if let Some(name) = name {
                ops.push(json!({ "op": "replace", "path": "/name", "value": name }));
            }
            if let Some(root_path) = root_path {
                ops.push(json!({ "op": "replace", "path": "/rootPath", "value": root_path }));
            }

            let client = make_client(&ctx, &[APP_UPDATE]).await?;
            let data = client
                .update_app(&app_id, Value::Array(ops))
                .await
                .map_err(client_err)?;

            Ok(CommandResult::new(data))
        },
    )
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    fn make_patch(name: Option<&str>, root_path: Option<&str>) -> Vec<serde_json::Value> {
        let mut ops = Vec::new();
        if let Some(n) = name {
            ops.push(json!({ "op": "replace", "path": "/name", "value": n }));
        }
        if let Some(p) = root_path {
            ops.push(json!({ "op": "replace", "path": "/rootPath", "value": p }));
        }
        ops
    }

    #[test]
    fn patch_with_name_only() {
        let ops = make_patch(Some("new-name"), None);
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0]["path"], "/name");
        assert_eq!(ops[0]["value"], "new-name");
        assert_eq!(ops[0]["op"], "replace");
    }

    #[test]
    fn patch_with_root_path_only() {
        let ops = make_patch(None, Some("/src"));
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0]["path"], "/rootPath");
        assert_eq!(ops[0]["value"], "/src");
    }

    #[test]
    fn patch_with_both_fields() {
        let ops = make_patch(Some("n"), Some("/p"));
        assert_eq!(ops.len(), 2);
    }
}
