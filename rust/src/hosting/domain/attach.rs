use cli_engine::{CommandResult, CommandSpec, NextActionParam, RuntimeCommandSpec, Tier};

use crate::hosting::common::{HostingDomain, client_err, make_client};
use crate::idn::DomainFormArgs;
use crate::next_action::next_action;
use crate::scopes::HOSTING_DOMAIN_WRITE as DOMAIN_WRITE;

#[derive(Debug, Clone, clap::Args)]
struct DomainAttachArgs {
    /// Application ID.
    #[arg(long = "app-id", value_name = "APP_ID")]
    app_id: String,

    /// Fully-qualified domain name to attach (e.g. www.example.com).
    #[arg(long, value_name = "HOSTNAME", value_parser = parse_hostname_arg)]
    hostname: String,

    #[command(flatten)]
    domain_format: DomainFormArgs,
}

/// Clap value-parser: a Unicode hostname is sent in its ASCII (punycode) form.
fn parse_hostname_arg(raw: &str) -> Result<String, String> {
    crate::idn::to_ascii(raw).map_err(|e| e.to_string())
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
            let client = make_client(&ctx, &[DOMAIN_WRITE]).await?;
            let mut data = client
                .attach_domain(&app_id, &args.hostname)
                .await
                .map_err(client_err)?;
            args.domain_format.form().convert_domain_fields(&mut data);
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
