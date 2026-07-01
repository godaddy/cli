//! `gddy domain contacts` — manage the local default-contacts file used by
//! `domain quote`/`purchase`. This is a local-file command group; no API/auth.

use cli_engine::{
    CliCoreError, CommandResult, CommandSpec, GroupSpec, NextAction, RuntimeCommandSpec,
    RuntimeGroupSpec, Tier,
};
use serde_json::json;

use crate::contacts;

pub(super) fn group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(
        GroupSpec::new(
            "contacts",
            "Manage saved default contacts for domain purchases",
        )
        .with_long(
            "Manage the optional contacts.toml that supplies registrant/admin/billing/\
                 tech contacts for `gddy domain purchase`. When a role is absent the \
                 purchase falls back to your account's default contact for that role.",
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
            if existed && !force {
                return Err(CliCoreError::message(format!(
                    "{} already exists; pass --force to overwrite",
                    path.display()
                )));
            }
            cli_engine::fs::write_string_atomic(&path, contacts::sample_toml())?;
            Ok(CommandResult::new(json!({
                "path": path.display().to_string(),
                "action": if existed { "overwritten" } else { "created" },
            }))
            .with_next_actions(vec![NextAction::new(
                "guide domain-purchase",
                "Learn how purchase uses these contacts",
            )]))
        },
    ))
}
