use cli_engine::{CommandResult, CommandSpec, NextActionParam, RuntimeCommandSpec, Tier};

use crate::hosting::common::{HostingSubscriptionAttachment, client_err, make_client};
use crate::next_action::next_action;
use crate::scopes::HOSTING_SUBSCRIPTION_WRITE as SUB_WRITE;

#[derive(Debug, Clone, clap::Args)]
struct SubscriptionAttachArgs {
    /// Application ID.
    #[arg(long = "app-id", value_name = "APP_ID")]
    app_id: String,

    /// Subscription ID of the hosting plan to attach.
    #[arg(long = "subscription-id", value_name = "SUBSCRIPTION_ID")]
    subscription_id: String,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<SubscriptionAttachArgs, _, _, _>(
        CommandSpec::from_args::<SubscriptionAttachArgs>(
            "attach",
            "Attach a hosting plan to an application",
        )
        .with_long(
            "Attach a hosting plan subscription to an application. \
             Use `hosting subscription list` to find available subscription IDs.",
        )
        .with_system("hosting")
        .with_tier(Tier::Mutate)
        .mutates(true)
        .with_scopes(&[SUB_WRITE])
        .with_output_schema::<HostingSubscriptionAttachment>(),
        |ctx, args: SubscriptionAttachArgs| async move {
            let app_id = args.app_id.clone();
            let client = make_client(&ctx, &[SUB_WRITE]).await?;
            let data = client
                .attach_subscription(&app_id, &args.subscription_id)
                .await
                .map_err(client_err)?;
            Ok(CommandResult::new(data).with_next_actions(vec![
                next_action(
                    "hosting subscription get --app-id <app-id>",
                    "Verify the attached subscription",
                )
                .with_param("app-id", NextActionParam::value(app_id)),
            ]))
        },
    )
}
