mod common;
mod create;
mod create_keys;
mod delete;
mod get;
mod list;
mod publish;
mod restart;
mod status;
mod unpublish;
mod update;

use cli_engine::{GroupSpec, RuntimeGroupSpec};

pub(super) fn group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(
        GroupSpec::new(
            "app",
            "Create, inspect, update, and delete hosting applications",
        )
        .with_alias("apps")
        .with_long(
            "Work with hosting applications of each hosting type: Node.js apps (NODEJS) \
             and WordPress sites (MHWP).",
        ),
    )
    .with_command(list::command())
    .with_command(get::command())
    .with_command(create::command())
    .with_command(update::command())
    .with_command(delete::command())
    .with_command(status::command())
    .with_command(restart::command())
    .with_command(publish::command())
    .with_command(unpublish::command())
}

#[cfg(test)]
mod tests {
    use clap::ValueEnum;

    use crate::hosting::common::HostingAppType;
    use crate::hosting::test_support::{cli, run_hosting};

    #[tokio::test]
    async fn app_help_names_both_app_types_and_leaves_details_to_the_commands() {
        let help = run_hosting(&cli(), &["app", "--help"]).await;
        let group_text = help.split("Usage:").next().unwrap_or_default();
        assert!(
            group_text.contains("Node.js apps (NODEJS) and WordPress sites (MHWP)"),
            "{group_text}"
        );
        // A new app type must be named here too.
        for app_type in HostingAppType::value_variants() {
            assert!(
                group_text.contains(&format!("({})", app_type.as_str())),
                "{app_type:?} is missing: {group_text}"
            );
        }
        // Each command's own help says what it does for a WordPress site, and only
        // `create` and `list` take --app-type, so the group names neither.
        for detail in [
            "placeholder page",
            "renames a WordPress site",
            "--root-path",
            "--app-type",
        ] {
            assert!(!group_text.contains(detail), "{detail:?}: {group_text}");
        }
    }
}
