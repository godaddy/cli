use cli_engine::{CommandResult, CommandSpec, NextActionParam, RuntimeCommandSpec, Tier};
use serde_json::json;

use crate::hosting::common::{client_err, make_client};
use crate::next_action::next_action;
use crate::scopes::HOSTING_DOMAIN_WRITE as DOMAIN_WRITE;

#[derive(Debug, Clone, clap::Args)]
struct DomainDetachArgs {
    /// Application ID.
    #[arg(long = "app-id", value_name = "APP_ID")]
    app_id: String,

    /// Domain ID.
    #[arg(long = "domain-id", value_name = "DOMAIN_ID")]
    domain_id: String,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<DomainDetachArgs, _, _, _>(
        CommandSpec::from_args::<DomainDetachArgs>("detach", "Detach a domain from an application")
            .with_long("Remove a domain from a hosting application. DNS records are not modified.")
            .with_system("hosting")
            .with_tier(Tier::Destructive)
            .mutates(true)
            .with_scopes(&[DOMAIN_WRITE]),
        |ctx, args: DomainDetachArgs| async move {
            let app_id = args.app_id.clone();
            let domain_id = args.domain_id.clone();
            let client = make_client(&ctx, &[DOMAIN_WRITE]).await?;
            client
                .detach_domain(&app_id, &domain_id)
                .await
                .map_err(client_err)?;
            Ok(
                CommandResult::new(json!({ "detached": true, "domainId": domain_id }))
                    .with_next_actions(vec![
                        next_action(
                            "hosting domain list --app-id <app-id>",
                            "View remaining domains",
                        )
                        .with_param("app-id", NextActionParam::value(app_id)),
                    ]),
            )
        },
    )
}
