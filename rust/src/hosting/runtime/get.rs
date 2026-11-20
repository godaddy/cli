use cli_engine::{CommandResult, CommandSpec, NextActionParam, RuntimeCommandSpec, Tier};

use crate::hosting::common::{AppIdArgs, HostingRuntime, client_err, make_client};
use crate::next_action::next_action;
use crate::scopes::HOSTING_APPLICATION_READ as APP_READ;

const GET_LONG: &str = "Get the runtime name and version for a hosting application. \
     The API returns an array of runtime/version objects \
     (e.g. runtime=NODEJS, version=22.11.0). \
     A WordPress (MHWP) site reports PHP and WORDPRESS. \
     An empty list is not an error. It means no version is known yet. \
     An older site may list WORDPRESS only.";

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<AppIdArgs, _, _, _>(
        CommandSpec::from_args::<AppIdArgs>("get", "Get application runtime configuration")
            .with_long(GET_LONG)
            .with_system("hosting")
            .with_tier(Tier::Read)
            .with_scopes(&[APP_READ])
            .with_output_schema::<HostingRuntime>(),
        |ctx, args: AppIdArgs| async move {
            let app_id = args.app_id;
            let client = make_client(&ctx, &[APP_READ]).await?;
            let data = client.get_runtime(&app_id).await.map_err(client_err)?;
            Ok(CommandResult::new(data).with_next_actions(vec![
                next_action(
                    "hosting app status --app-id <app-id>",
                    "Check application status",
                )
                .with_param("app-id", NextActionParam::value(app_id)),
            ]))
        },
    )
}

#[cfg(test)]
mod tests {
    use cli_engine::Cli;

    use crate::hosting::test_support::{cli, run_hosting};

    async fn run(cli: &Cli, args: &[&str]) -> String {
        let mut argv = vec!["runtime"];
        argv.extend_from_slice(args);
        run_hosting(cli, &argv).await
    }

    #[tokio::test]
    async fn runtime_help_names_php_and_wordpress_and_an_empty_list() {
        let help = run(&cli(), &["get", "--help"]).await;
        assert!(help.contains("PHP"), "{help}");
        assert!(help.contains("WORDPRESS"), "{help}");
        assert!(help.contains("empty list"), "{help}");
    }
}
