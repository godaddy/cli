use cli_engine::{CommandResult, CommandSpec, NextActionParam, RuntimeCommandSpec, Tier};

use crate::hosting::common::{HostingDomain, client_err, make_client};
use crate::next_action::next_action;
use crate::scopes::HOSTING_DOMAIN_READ as DOMAIN_READ;

#[derive(Debug, Clone, clap::Args)]
struct DomainGetArgs {
    /// Application ID.
    #[arg(long = "app-id", value_name = "APP_ID")]
    app_id: String,

    /// Domain ID.
    #[arg(long = "domain-id", value_name = "DOMAIN_ID")]
    domain_id: String,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<DomainGetArgs, _, _, _>(
        CommandSpec::from_args::<DomainGetArgs>("get", "Get an attached domain")
            .with_long("Get the details and status of a domain attached to a hosting application.")
            .with_system("hosting")
            .with_tier(Tier::Read)
            .with_scopes(&[DOMAIN_READ])
            .with_output_schema::<HostingDomain>(),
        |ctx, args: DomainGetArgs| async move {
            let app_id = args.app_id.clone();
            let domain_id = args.domain_id.clone();
            let client = make_client(&ctx, &[DOMAIN_READ]).await?;
            let data = client
                .get_domain(&app_id, &domain_id)
                .await
                .map_err(client_err)?;
            Ok(CommandResult::new(data).with_next_actions(vec![
                next_action(
                    "hosting domain detach --app-id <app-id> --domain-id <domain-id>",
                    "Detach this domain",
                )
                .with_param("app-id", NextActionParam::value(app_id))
                .with_param("domain-id", NextActionParam::value(domain_id)),
            ]))
        },
    )
}
