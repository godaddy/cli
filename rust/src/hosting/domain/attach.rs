use cli_engine::{CommandResult, CommandSpec, NextActionParam, RuntimeCommandSpec, Tier};

use crate::hosting::common::{HostingDomain, client_err, make_client};
use crate::next_action::next_action;
use crate::scopes::HOSTING_DOMAIN_WRITE as DOMAIN_WRITE;

#[derive(Debug, Clone, clap::Args)]
struct DomainAttachArgs {
    /// Application ID.
    #[arg(long = "app-id", value_name = "APP_ID")]
    app_id: String,

    /// Fully-qualified domain name to attach (e.g. www.example.com).
    #[arg(long, value_name = "HOSTNAME")]
    hostname: String,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<DomainAttachArgs, _, _, _>(
        CommandSpec::from_args::<DomainAttachArgs>("attach", "Attach a domain to an application")
            .with_long(
                "Attach a fully-qualified domain name to a hosting application. \
                 The domain must be registered and have DNS pointing to GoDaddy hosting. \
                 Returns immediately — poll `hosting domain get` until status is ACTIVE.",
            )
            .with_system("hosting")
            .with_tier(Tier::Mutate)
            .mutates(true)
            .with_scopes(&[DOMAIN_WRITE])
            .with_output_schema::<HostingDomain>(),
        |ctx, args: DomainAttachArgs| async move {
            let app_id = args.app_id.clone();
            let client = make_client(&ctx, &[DOMAIN_WRITE]).await?;
            let data = client
                .attach_domain(&app_id, &args.hostname)
                .await
                .map_err(client_err)?;
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
