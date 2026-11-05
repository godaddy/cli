use cli_engine::{CommandResult, CommandSpec, RuntimeCommandSpec, Tier};
use serde_json::{Value, json};

use crate::error::GddyError;
use crate::hosting::client::ClientError;
use crate::hosting::common::{
    HostingApplication, client_err_retry_on_503, client_err_with_fix, error_issues, is_mhwp_app_id,
    make_client,
};
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

fn update_long(mhwp: bool) -> String {
    let mut long = "Update application metadata. At least one of --name or --root-path \
         is required. Changes take effect immediately and do not trigger a \
         new deployment."
        .to_owned();
    if mhwp {
        long.push_str(
            "\n\nA WordPress (MHWP) site can only be renamed. --root-path does not apply, \
             and passing it refuses the whole update, including a --name in the same \
             command. --name is the label in the CLI and in Airo App Builder. It does not \
             become the WordPress site title. The name is 1–200 characters and cannot \
             contain control characters or '<' or '>'.",
        );
    }
    long
}

pub(super) fn command(mhwp: bool) -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<AppUpdateArgs, _, _, _>(
        CommandSpec::from_args::<AppUpdateArgs>("update", "Update a hosting application")
            .with_long(update_long(mhwp))
            .with_system("hosting")
            .with_tier(Tier::Mutate)
            .mutates(true)
            .with_scopes(&[APP_UPDATE])
            .with_output_schema::<HostingApplication>(),
        |ctx, args: AppUpdateArgs| async move {
            let app_id = args.app_id;
            let ops = patch_ops(args.name, args.root_path).ok_or_else(|| {
                GddyError::validation("at least one of --name or --root-path is required")
                    .into_cli_error()
            })?;

            let client = make_client(&ctx, &[APP_UPDATE]).await?;
            let data = client
                .update_app(&app_id, Value::Array(ops))
                .await
                .map_err(|e| update_err(e, is_mhwp_app_id(&app_id)))?;

            Ok(CommandResult::new(data))
        },
    )
}

/// The JSON Patch for the flags given, or `None` when none carries a value.
fn patch_ops(name: Option<String>, root_path: Option<String>) -> Option<Vec<Value>> {
    let mut ops = Vec::new();
    if let Some(name) = name.filter(|s| !s.trim().is_empty()) {
        ops.push(json!({ "op": "replace", "path": "/name", "value": name }));
    }
    if let Some(root_path) = root_path.filter(|s| !s.trim().is_empty()) {
        ops.push(json!({ "op": "replace", "path": "/rootPath", "value": root_path }));
    }
    (!ops.is_empty()).then_some(ops)
}

/// The service refuses a WordPress patch that touches `/rootPath` as a whole,
/// so a rename sent beside it is not applied either.
fn update_err(e: ClientError, mhwp: bool) -> cli_engine::CliCoreError {
    let fix = error_issues(&e).iter().find_map(|issue| match issue.as_str() {
        "APP_DELETING" if mhwp => Some(
            "This site is being deleted. Wait until `hosting app get` reports it was not found.",
        ),
        "ROOT_PATH_NOT_APPLICABLE" if mhwp => Some(
            "Run the command again with only --name. --root-path does not apply to a WordPress site.",
        ),
        _ => None,
    });
    match fix {
        Some(fix) => client_err_with_fix(e, fix),
        None => client_err_retry_on_503(e, mhwp),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hosting::test_support::{cli, run_hosting};

    #[test]
    fn patch_with_name_only() {
        let ops = patch_ops(Some("new-name".to_owned()), None).expect("ops");
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0]["path"], "/name");
        assert_eq!(ops[0]["value"], "new-name");
        assert_eq!(ops[0]["op"], "replace");
    }

    #[test]
    fn patch_with_root_path_only() {
        let ops = patch_ops(None, Some("/src".to_owned())).expect("ops");
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0]["path"], "/rootPath");
        assert_eq!(ops[0]["value"], "/src");
    }

    #[test]
    fn patch_with_both_fields() {
        let ops = patch_ops(Some("n".to_owned()), Some("/p".to_owned())).expect("ops");
        assert_eq!(ops.len(), 2);
    }

    #[test]
    fn patch_ignores_blank_values() {
        assert!(patch_ops(None, None).is_none());
        assert!(patch_ops(Some("  ".to_owned()), Some(String::new())).is_none());
    }

    #[tokio::test]
    async fn update_help_says_a_wordpress_site_only_renames() {
        let on = run_hosting(&cli(true), &["app", "update", "--help"]).await;
        assert!(on.contains("can only be renamed"), "{on}");
        assert!(
            on.contains("does not become the WordPress site title"),
            "{on}"
        );

        let off = run_hosting(&cli(false), &["app", "update", "--help"]).await;
        assert!(!off.contains("MHWP"), "{off}");
        assert!(!off.contains("WordPress"), "{off}");
    }

    fn fix_for(status: u16, issue: &str, mhwp: bool) -> Option<String> {
        let err = update_err(
            ClientError::Http {
                status,
                body: format!(r#"{{"message":"m","details":[{{"issue":"{issue}"}}]}}"#),
            },
            mhwp,
        );
        cli_engine::build_error_envelope(&err, "hosting").fix
    }

    #[test]
    fn wordpress_refusals_have_fixes() {
        let deleting = fix_for(422, "APP_DELETING", true);
        assert!(
            deleting
                .as_deref()
                .is_some_and(|f| f.contains("being deleted")),
            "{deleting:?}"
        );
        let root = fix_for(422, "ROOT_PATH_NOT_APPLICABLE", true);
        assert!(
            root.as_deref()
                .is_some_and(|f| f.contains("only --name") && f.contains("--root-path")),
            "{root:?}"
        );
    }

    #[test]
    fn nodejs_does_not_get_the_wordpress_fixes() {
        for (status, issue) in [
            (422, "APP_DELETING"),
            (422, "ROOT_PATH_NOT_APPLICABLE"),
            (503, "UPSTREAM_UNAVAILABLE"),
        ] {
            let fix = fix_for(status, issue, false);
            assert!(
                fix.as_deref().is_some_and(
                    |f| !f.contains("WordPress") && !f.contains("temporarily unavailable")
                ),
                "{issue}: {fix:?}"
            );
        }
    }
}
