//! `gddy platform app validate` — check remote application state.

use cli_engine::{CommandResult, CommandSpec, NextActionParam, RuntimeCommandSpec, Tier};
use platform_app_client::application::{ApplicationApplication, ApplicationStatus};
use serde_json::json;

use super::schemas::ValidationResult;
use crate::next_action::{next_action, required_value};

/// Missing URL is an error; missing proxy URL or INACTIVE status are warnings.
fn validate_remote_application(app: &ApplicationApplication) -> (bool, Vec<String>, Vec<String>) {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    let url = app.url.as_deref().unwrap_or("");
    if url.is_empty() {
        errors.push("Application URL is required".to_owned());
    }
    let proxy_url = app.proxy_url.as_deref().unwrap_or("");
    if proxy_url.is_empty() {
        warnings.push("Proxy URL is not set".to_owned());
    }
    if app.status == ApplicationStatus::INACTIVE {
        warnings.push("Application is currently inactive".to_owned());
    }

    (errors.is_empty(), errors, warnings)
}

#[derive(Debug, Clone, clap::Args)]
pub(super) struct ValidateArgs {
    /// Application name.
    #[arg(value_name = "NAME")]
    pub(super) name: String,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<ValidateArgs, _, _, _>(
        CommandSpec::from_args::<ValidateArgs>("validate", "Validate remote application state")
            .with_long(
                "Fetch a GoDaddy developer-platform application by name and \
                validate its remote configuration. Reports an error when the \
                application URL is missing and warnings when the proxy URL is \
                unset or the application is inactive.",
            )
            .with_system("applications")
            .with_tier(Tier::Read)
            .with_output_schema::<ValidationResult>(),
        |ctx, args: ValidateArgs| async move {
            let name = args.name;
            let client = super::make_client(&ctx).await?;
            let app = client
                .get_application(&name)
                .await
                .map_err(super::client_err)?
                .ok_or_else(|| {
                    crate::error::GddyError::not_found(format!("application '{name}' not found"))
                        .into_cli_error()
                })?;

            let app_id = app.id.clone();
            let (valid, errors, warnings) = validate_remote_application(&app);

            // Only suggest a release once the app is valid; otherwise point back to
            // `info` to review the reported problems.
            let mut next_actions = Vec::new();
            if valid {
                next_actions.push(
                    next_action(
                        "platform app release --application-id <application-id> --version <version>",
                        "Create a release after validation",
                    )
                    .with_param("application-id", required_value(&app_id))
                    .with_param("version", NextActionParam::required()),
                );
            }
            next_actions.push(
                next_action(
                    "platform app info --name <name>",
                    "Inspect application details",
                )
                .with_param("name", required_value(&name)),
            );

            Ok(CommandResult::new(json!({
                "valid": valid,
                "errors": errors,
                "warnings": warnings,
            }))
            .with_next_actions(next_actions))
        },
    )
}

#[cfg(test)]
mod tests {
    use platform_app_client::application::{ApplicationApplication, ApplicationStatus};

    use super::validate_remote_application;

    fn validate_clap_command() -> clap::Command {
        super::command().spec.clap_command()
    }

    fn app(
        url: &str,
        proxy_url: Option<&str>,
        status: ApplicationStatus,
    ) -> ApplicationApplication {
        ApplicationApplication {
            id: "app-1".to_owned(),
            label: "My App".to_owned(),
            name: "my-app".to_owned(),
            description: None,
            status,
            url: Some(url.to_owned()),
            proxy_url: proxy_url.map(str::to_owned),
        }
    }

    #[test]
    fn validate_requires_name() {
        let err = validate_clap_command()
            .try_get_matches_from(["validate"])
            .expect_err("validate without name should be rejected");
        assert_eq!(
            err.kind(),
            clap::error::ErrorKind::MissingRequiredArgument,
            "expected MissingRequiredArgument, got: {err}"
        );
    }

    #[test]
    fn validate_accepts_positional_name() {
        validate_clap_command()
            .try_get_matches_from(["validate", "my-app"])
            .expect("positional name should be accepted");
    }

    #[test]
    fn validate_remote_healthy_app_is_valid() {
        let (valid, errors, warnings) = validate_remote_application(&app(
            "https://example.com",
            Some("https://proxy.example.com"),
            ApplicationStatus::ACTIVE,
        ));
        assert!(valid);
        assert!(errors.is_empty());
        assert!(warnings.is_empty());
    }

    #[test]
    fn validate_remote_missing_url_is_error() {
        let (valid, errors, warnings) = validate_remote_application(&app(
            "",
            Some("https://proxy.example.com"),
            ApplicationStatus::ACTIVE,
        ));
        assert!(!valid);
        assert_eq!(errors, vec!["Application URL is required".to_owned()]);
        assert!(warnings.is_empty());
    }

    #[test]
    fn validate_remote_missing_proxy_and_inactive_are_warnings() {
        let (valid, errors, warnings) = validate_remote_application(&app(
            "https://example.com",
            None,
            ApplicationStatus::INACTIVE,
        ));
        assert!(valid, "warnings alone should not invalidate");
        assert!(errors.is_empty());
        assert_eq!(
            warnings,
            vec![
                "Proxy URL is not set".to_owned(),
                "Application is currently inactive".to_owned(),
            ]
        );
    }
}
