//! `gddy domain contacts` — manage the local default-contacts file used by
//! `domain quote`/`purchase`. This is a local-file command group; no API/auth.

use cli_engine::{
    CliCoreError, CommandResult, CommandSpec, GroupSpec, RuntimeCommandSpec, RuntimeGroupSpec, Tier,
};
use serde_json::json;

use crate::contacts;
use crate::next_action::next_action;

pub(super) fn group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(
        GroupSpec::new(
            "contacts",
            "Manage saved default contacts for domain purchases",
        )
        .with_long(
            "Manage the optional contacts.toml that supplies registrant/admin/billing/\
                 tech contacts for a registration. These are read at `gddy domain quote` \
                 time and bound into the quote token, so `gddy domain purchase` registers \
                 with exactly the contacts you quoted — edit contacts.toml and re-quote to \
                 change them. When a role is absent the registration falls back to your \
                 account's default contact for that role.",
        ),
    )
    .with_command(RuntimeCommandSpec::new_with_context(
        CommandSpec::new("init", "Write a starter contacts.toml you can edit")
            .with_long(
                "Scaffold a starter contacts.toml in your config directory with \
                     every role commented out (so it's inert until you edit it). Fill \
                     in any roles you want to override the account defaults for. Pass \
                     --force to overwrite an existing file.",
            )
            .with_system("domain")
            .with_tier(Tier::Mutate)
            .mutates(true)
            .handles_dry_run(true)
            .no_auth(true)
            .with_default_fields("path,action")
            .with_arg(
                clap::Arg::new("force")
                    .long("force")
                    .action(clap::ArgAction::SetTrue)
                    .help("Overwrite an existing contacts.toml"),
            ),
        |ctx| async move {
            let path = contacts::contacts_path().ok_or_else(|| {
                CliCoreError::message("could not determine a config directory for contacts.toml")
            })?;
            let force = ctx
                .args
                .get("force")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let existed = path.exists();
            // Runs unconditionally, including under `--dry-run`, so a missing
            // `--force` against an existing file is still reported as an error
            // rather than a silent "would execute".
            let action = init_action(&path, existed, force)?;
            if ctx.dry_run() {
                let preview_action = if existed {
                    "would overwrite"
                } else {
                    "would create"
                };
                return Ok(CommandResult::new(json!({
                    "path": path.display().to_string(),
                    "action": preview_action,
                }))
                .with_dry_run());
            }
            cli_engine::fs::write_string_atomic(&path, contacts::sample_toml())?;
            Ok(CommandResult::new(json!({
                "path": path.display().to_string(),
                "action": action,
            }))
            .with_next_actions(vec![next_action(
                "guide domain-purchase",
                "Learn how purchase uses these contacts",
            )]))
        },
    ))
}

/// Decides what `contacts init` would report, or the error it would return,
/// given whether the file exists and `--force`. Shared by the real execution
/// and dry-run paths so both agree on when this command would fail.
fn init_action(
    path: &std::path::Path,
    existed: bool,
    force: bool,
) -> Result<&'static str, CliCoreError> {
    if existed && !force {
        return Err(CliCoreError::message(format!(
            "{} already exists; pass --force to overwrite",
            path.display()
        )));
    }
    Ok(if existed { "overwritten" } else { "created" })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use cli_engine::{Cli, CliConfig};

    use super::init_action;

    #[test]
    fn errors_when_file_exists_and_force_is_not_passed() {
        let err = init_action(Path::new("/tmp/contacts.toml"), true, false)
            .expect_err("should refuse to overwrite without --force");
        assert!(err.to_string().contains("--force"));
    }

    #[test]
    fn overwrites_when_file_exists_and_force_is_passed() {
        assert_eq!(
            init_action(Path::new("/tmp/contacts.toml"), true, true).expect("should succeed"),
            "overwritten"
        );
    }

    #[test]
    fn creates_when_file_does_not_exist_regardless_of_force() {
        assert_eq!(
            init_action(Path::new("/tmp/contacts.toml"), false, false).expect("should succeed"),
            "created"
        );
        assert_eq!(
            init_action(Path::new("/tmp/contacts.toml"), false, true).expect("should succeed"),
            "created"
        );
    }

    /// `--dry-run` never reaches `write_string_atomic` (it returns before that
    /// call), so this is safe to run against the real config dir: it can only
    /// read `path.exists()`, never write.
    #[tokio::test]
    async fn init_dry_run_previews_without_writing() {
        let cli = Cli::new(
            CliConfig::new("gddy", "GoDaddy developer CLI", "gddy")
                .with_module(crate::domain::module()),
        );
        // `--force` so the assertion holds regardless of whether this
        // machine already has a contacts.toml (the case this test exists to
        // cover in the first place: DEVEX-890).
        let output = cli
            .run([
                "gddy",
                "domain",
                "contacts",
                "init",
                "--force",
                "--dry-run",
                "--output",
                "json",
            ])
            .await;

        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let rendered: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json");
        let action = rendered["data"]["action"].as_str().unwrap_or_default();
        assert!(
            action == "would create" || action == "would overwrite",
            "unexpected action: {rendered}"
        );
    }
}
