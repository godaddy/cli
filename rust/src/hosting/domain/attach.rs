use cli_engine::{CommandResult, CommandSpec, NextActionParam, RuntimeCommandSpec, Tier};

use crate::hosting::client::ClientError;
use crate::hosting::common::{
    HostingDomain, client_err, client_err_with_fix, error_issues, make_client,
};
use crate::idn::DomainFormArgs;
use crate::next_action::next_action;
use crate::scopes::HOSTING_DOMAIN_WRITE as DOMAIN_WRITE;

#[derive(Debug, Clone, clap::Args)]
struct DomainAttachArgs {
    /// Application ID.
    #[arg(long = "app-id", value_name = "APP_ID")]
    app_id: String,

    /// Fully-qualified domain name to attach (e.g. www.example.com).
    #[arg(long, value_name = "HOSTNAME", value_parser = crate::idn::parse_name_arg)]
    hostname: String,

    #[command(flatten)]
    domain_format: DomainFormArgs,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<DomainAttachArgs, _, _, _>(
        CommandSpec::from_args::<DomainAttachArgs>("attach", "Attach a domain to an application")
            .with_long(
                "Attach a fully-qualified domain name to a hosting application. \
                 Returns immediately. Poll `hosting domain get` until verificationStatus \
                 is ACTIVE. For CUSTOM domains whose DNS is not on GoDaddy, use \
                 certificateValidationCname as a `_acme-challenge` CNAME target and \
                 anycastIp as the hostname's A record once those fields are non-null.",
            )
            .with_system("hosting")
            .with_tier(Tier::Mutate)
            .mutates(true)
            .with_scopes(&[DOMAIN_WRITE])
            .with_output_schema::<HostingDomain>(),
        |ctx, args: DomainAttachArgs| async move {
            let app_id = args.app_id.clone();
            // Resolve the display form before attaching, so a bad preference
            // can't hide the details of an attachment that already succeeded.
            let domain_format = args.domain_format.form(&ctx)?;
            let client = make_client(&ctx, &[DOMAIN_WRITE]).await?;
            let mut data = client
                .attach_domain(&app_id, &args.hostname)
                .await
                .map_err(attach_err)?;
            domain_format.convert_fields(&mut data, &["hostname"]);
            Ok(CommandResult::new(data).with_next_actions(vec![
                next_action(
                    "hosting domain list --app-id <app-id>",
                    "View all attached domains",
                )
                .with_param("app-id", NextActionParam::value(app_id)),
            ]))
        },
    )
}

/// An app on Airo App Builder takes a domain only once it is live, so that
/// refusal names the step to run first.
fn attach_err(e: ClientError) -> cli_engine::CliCoreError {
    if error_issues(&e).iter().any(|i| i == "APP_NOT_PUBLISHED") {
        return client_err_with_fix(
            e,
            "Take the app live first, then attach the domain. \
             Run: gddy hosting app publish --app-id <app-id>",
        );
    }
    client_err(e)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fix_for(issue: &str) -> Option<String> {
        let err = attach_err(ClientError::Http {
            status: 422,
            body: format!(r#"{{"message":"m","details":[{{"issue":"{issue}"}}]}}"#),
        });
        cli_engine::build_error_envelope(&err, "hosting").fix
    }

    #[test]
    fn an_app_that_is_not_live_is_told_to_publish_first() {
        let fix = fix_for("APP_NOT_PUBLISHED");
        assert!(
            fix.as_deref()
                .is_some_and(|f| f.contains("gddy hosting app publish --app-id <app-id>")),
            "{fix:?}"
        );
        let fix = fix_for("DOMAIN_ALREADY_ATTACHED");
        assert!(
            fix.as_deref().is_some_and(|f| !f.contains("app publish")),
            "{fix:?}"
        );
    }
}
