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

use crate::hosting::common::SUPPORTED_APP_TYPES;

pub(super) fn group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(
        GroupSpec::new(
            "app",
            "Create, inspect, update, and delete hosting applications",
        )
        .with_alias("apps")
        .with_long(format!(
            "Work with hosting applications. Use --app-type on list and create to \
             specify the application type (currently {SUPPORTED_APP_TYPES}). \
             `publish` takes any app live; `unpublish` takes a WordPress (MHWP) site back \
             to a placeholder page. `update` renames a WordPress site; `--root-path` does \
             not apply to it."
        )),
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
    use crate::hosting::test_support::{cli, run_hosting};

    #[tokio::test]
    async fn app_help_names_both_app_types_and_the_wordpress_limits() {
        let help = run_hosting(&cli(), &["app", "--help"]).await;
        assert!(help.contains("currently NODEJS, MHWP"), "{help}");
        assert!(help.contains("renames a WordPress site"), "{help}");
    }
}
