use cli_engine::{CommandResult, NextActionParam, RuntimeCommandSpec, Tier};
use serde_json::json;

use crate::hosting::client::ClientError;
use crate::hosting::common::{
    HostingAppOperation, HostingAppType, app_type_command, client_err, client_err_with_fix,
    error_issues, make_client,
};
use crate::next_action::next_action;
use crate::scopes::HOSTING_APPLICATION_CREATE as APP_CREATE;

#[derive(Debug, Clone, clap::Args)]
struct AppCreateArgs {
    /// Hosting product.
    #[arg(long = "app-type", value_name = "TYPE", ignore_case = true)]
    app_type: HostingAppType,

    /// Human-readable display name (1–200 characters).
    #[arg(long, value_name = "NAME")]
    name: String,
}

const MHWP_HELP: &str = "\n\nMHWP creates a WordPress site and needs an Airo App Builder \
     subscription. The site starts as Coming Soon, and its WordPress title is not set \
     from --name. It is hosted in the US. Log in to WordPress admin from Airo App Builder. \
     For MHWP apps, use `hosting app get`, `status`, `update --name`, `delete` and \
     `hosting runtime get`; other hosting commands do not apply.";

pub(super) fn command(mhwp: bool) -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<AppCreateArgs, _, _, _>(
        app_type_command::<AppCreateArgs>("create", "Create a hosting application", mhwp)
            .with_long(format!(
                "Provision a new hosting application slot. Because no app ID exists \
                 until provisioning completes, this returns an operation ID. \
                 Poll `hosting operation get --operation-id <id>` until \
                 status is COMPLETED or FAILED. On COMPLETED, the operation's \
                 `app` field carries the created app; use `app.id` \
                 as the --app-id for all subsequent calls.{}",
                if mhwp { MHWP_HELP } else { "" }
            ))
            .with_system("hosting")
            .with_tier(Tier::Mutate)
            .mutates(true)
            .with_scopes(&[APP_CREATE])
            .with_output_schema::<HostingAppOperation>(),
        |ctx, args: AppCreateArgs| async move {
            let app_type = args.app_type;
            let name = args.name;
            let client = make_client(&ctx, &[APP_CREATE]).await?;
            let data = client
                .create_app(app_type.as_str(), json!({ "name": name }))
                .await
                .map_err(|e| create_err(e, app_type))?;

            let operation_id = data
                .get("operationId")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_owned();

            let poll_description = poll_description(app_type);
            let mut poll_action = next_action(
                "hosting operation get --operation-id <operation-id>",
                poll_description,
            )
            .with_param("operation-id", NextActionParam::required());

            if !operation_id.is_empty() {
                poll_action = next_action(
                    "hosting operation get --operation-id <operation-id>",
                    poll_description,
                )
                .with_param("operation-id", NextActionParam::value(operation_id));
            }

            Ok(CommandResult::new(data).with_next_actions(vec![poll_action]))
        },
    )
}

fn poll_description(app_type: HostingAppType) -> &'static str {
    match app_type {
        HostingAppType::Nodejs => "Poll app provisioning status",
        HostingAppType::Mhwp => {
            "Poll app provisioning status. The WordPress site starts as Coming Soon; \
             log in to WordPress admin from Airo App Builder"
        }
    }
}

fn create_err(e: ClientError, app_type: HostingAppType) -> cli_engine::CliCoreError {
    let mhwp = app_type == HostingAppType::Mhwp;
    let fix = error_issues(&e).iter().find_map(|issue| match issue.as_str() {
        "SUBSCRIPTION_REQUIRED" if mhwp => Some(
            "MHWP needs an Airo App Builder subscription. Set one up, then run this command again."
                .to_owned(),
        ),
        "SUBSCRIPTION_INACTIVE" if mhwp => Some(
            "Your Airo App Builder subscription is not active. Reactivate it, then run this command again."
                .to_owned(),
        ),
        "WORDPRESS_NOT_ENABLED" if mhwp => Some(
            "WordPress apps are not enabled for this account yet. Contact GoDaddy support.".to_owned(),
        ),
        "MULTIPLE_SYSTEMS_AMBIGUOUS" if mhwp => Some(
            "Your Airo App Builder account has more than one hosting system. \
             Create the site in Airo App Builder instead."
                .to_owned(),
        ),
        "APP_LIMIT_EXCEEDED" => Some(format!(
            "You reached the app limit. Delete an app you no longer need, then run this command again. \
             Run: gddy hosting app list --app-type {}",
            app_type.as_str()
        )),
        _ => None,
    });
    match fix {
        Some(fix) => client_err_with_fix(e, fix),
        None => client_err(e),
    }
}

#[cfg(test)]
mod tests {
    use cli_engine::{Cli, CliConfig, Stage};

    use super::*;

    fn fix_for(status: u16, issue: &str, app_type: HostingAppType) -> Option<String> {
        let err = create_err(
            ClientError::Http {
                status,
                body: format!(r#"{{"message":"m","details":[{{"issue":"{issue}"}}]}}"#),
            },
            app_type,
        );
        cli_engine::build_error_envelope(&err, "hosting").fix
    }

    #[test]
    fn create_err_gives_a_fix_for_each_create_issue() {
        for (status, issue, expected) in [
            (
                422,
                "SUBSCRIPTION_REQUIRED",
                "needs an Airo App Builder subscription",
            ),
            (403, "SUBSCRIPTION_INACTIVE", "subscription is not active"),
            (403, "WORDPRESS_NOT_ENABLED", "not enabled for this account"),
            (
                422,
                "MULTIPLE_SYSTEMS_AMBIGUOUS",
                "more than one hosting system",
            ),
            (
                422,
                "APP_LIMIT_EXCEEDED",
                "hosting app list --app-type MHWP",
            ),
        ] {
            let fix = fix_for(status, issue, HostingAppType::Mhwp);
            assert!(
                fix.as_deref().is_some_and(|f| f.contains(expected)),
                "{issue}: {fix:?}"
            );
        }
    }

    #[test]
    fn create_err_finds_a_known_issue_after_other_details() {
        let err = create_err(
            ClientError::Http {
                status: 422,
                body: r#"{"message":"m","details":[{"issue":"VALIDATION_FAILED"},{"issue":"APP_LIMIT_EXCEEDED"}]}"#
                    .to_owned(),
            },
            HostingAppType::Mhwp,
        );
        let fix = cli_engine::build_error_envelope(&err, "hosting").fix;
        assert!(
            fix.as_deref().is_some_and(|f| f.contains("app limit")),
            "{fix:?}"
        );
    }

    #[test]
    fn create_err_keeps_default_fix_for_other_errors() {
        let fix = fix_for(403, "SOMETHING_ELSE", HostingAppType::Mhwp);
        assert!(
            fix.as_deref().is_some_and(|f| f.contains("auth scopes")),
            "{fix:?}"
        );
    }

    #[test]
    fn create_err_keeps_wordpress_hints_off_nodejs() {
        let fix = fix_for(403, "WORDPRESS_NOT_ENABLED", HostingAppType::Nodejs);
        assert!(
            fix.as_deref().is_some_and(|f| f.contains("auth scopes")),
            "{fix:?}"
        );
        let fix = fix_for(422, "APP_LIMIT_EXCEEDED", HostingAppType::Nodejs);
        assert!(
            fix.as_deref()
                .is_some_and(|f| f.contains("--app-type NODEJS")),
            "{fix:?}"
        );
    }

    #[test]
    fn poll_description_adds_wordpress_notes_for_mhwp() {
        assert_eq!(
            poll_description(HostingAppType::Nodejs),
            "Poll app provisioning status"
        );
        assert!(poll_description(HostingAppType::Mhwp).contains("Coming Soon"));
    }

    fn cli(mhwp: bool) -> Cli {
        let mut config = CliConfig::new("gddy", "GoDaddy developer CLI", "gddy")
            .with_min_stage(Stage::Beta)
            // Unregistered provider: commands that pass parsing stop at auth.
            .with_default_auth_provider("godaddy")
            .with_module(crate::hosting::module());
        if mhwp {
            config = config.with_feature_override("hosting-mhwp", Stage::Ga);
        }
        Cli::new(config)
    }

    async fn run(cli: &Cli, args: &[&str]) -> String {
        let mut argv = vec!["gddy", "hosting", "app"];
        argv.extend_from_slice(args);
        cli.run(argv).await.rendered
    }

    const REJECTED: &str = "invalid value 'MHWP'";
    const PARSED: &str = "no provider registered";

    #[tokio::test]
    async fn mhwp_is_rejected_before_auth_while_its_flag_is_off() {
        let cli = cli(false);
        let create = run(&cli, &["create", "--app-type", "MHWP", "--name", "s"]).await;
        assert!(create.contains(REJECTED), "{create}");
        let list = run(&cli, &["list", "--app-type", "MHWP"]).await;
        assert!(list.contains(REJECTED), "{list}");
        let nodejs = run(&cli, &["create", "--app-type", "nodejs", "--name", "s"]).await;
        assert!(nodejs.contains(PARSED), "{nodejs}");

        let help = run(&cli, &["create", "--help"]).await;
        assert!(
            help.contains("Provision a new hosting application"),
            "{help}"
        );
        assert!(!help.contains("MHWP"), "{help}");
    }

    #[tokio::test]
    async fn mhwp_is_accepted_and_documented_while_its_flag_is_on() {
        let cli = cli(true);
        let create = run(&cli, &["create", "--app-type", "mhwp", "--name", "s"]).await;
        assert!(create.contains(PARSED), "{create}");
        let list = run(&cli, &["list", "--app-type", "MHWP"]).await;
        assert!(list.contains(PARSED), "{list}");

        let help = run(&cli, &["create", "--help"]).await;
        assert!(help.contains("Airo App Builder subscription"), "{help}");
        assert!(
            help.contains("other hosting commands do not apply"),
            "{help}"
        );
        assert!(help.contains("possible values: NODEJS, MHWP"), "{help}");
    }
}
