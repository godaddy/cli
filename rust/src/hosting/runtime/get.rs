use cli_engine::{CommandResult, CommandSpec, NextActionParam, RuntimeCommandSpec, Tier};

use crate::hosting::common::{AppIdArgs, HostingRuntime, client_err, make_client};
use crate::next_action::next_action;
use crate::scopes::HOSTING_APPLICATION_READ as APP_READ;

fn get_long(mhwp: bool) -> String {
    let mut long = "Get the runtime name and version for a hosting application. \
         The API returns an array of runtime/version objects \
         (e.g. runtime=NODEJS, version=22.11.0)."
        .to_owned();
    if mhwp {
        long.push_str(
            " A WordPress (MHWP) site reports PHP and WORDPRESS. \
             An empty array means no version is known yet, which is a successful response. \
             An older site may list WORDPRESS only.",
        );
    }
    long
}

pub(super) fn command(mhwp: bool) -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<AppIdArgs, _, _, _>(
        CommandSpec::from_args::<AppIdArgs>("get", "Get application runtime configuration")
            .with_long(get_long(mhwp))
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
    async fn runtime_help_names_php_and_wordpress_and_an_empty_array() {
        let on = run(&cli(true), &["get", "--help"]).await;
        assert!(on.contains("PHP"), "{on}");
        assert!(on.contains("WORDPRESS"), "{on}");
        assert!(on.contains("empty array"), "{on}");

        let off = run(&cli(false), &["get", "--help"]).await;
        assert!(!off.contains("MHWP"), "{off}");
        assert!(!off.contains("WORDPRESS"), "{off}");
    }
}
