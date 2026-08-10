//! `gddy platform app add` — append actions and webhook subscriptions to
//! godaddy.toml. The `extension` subgroup lives in [`super::add_extension`].

use cli_engine::{
    CommandResult, CommandSpec, GroupSpec, RuntimeCommandSpec, RuntimeGroupSpec, Tier,
};
use serde_json::json;

use super::schemas::{ConfigAction, ConfigSubscription};

#[derive(Debug, Clone, clap::Args)]
struct ActionArgs {
    /// Unique action name written into godaddy.toml.
    #[arg(long)]
    name: String,

    /// Public HTTPS URL the platform will invoke for this action.
    #[arg(long)]
    url: String,
}

#[derive(Debug, Clone, clap::Args)]
struct SubscriptionArgs {
    /// Unique subscription name written into godaddy.toml.
    #[arg(long)]
    name: String,

    /// Public HTTPS URL that will receive webhook POST requests.
    #[arg(long)]
    url: String,

    /// One or more event types to subscribe to (run `gddy platform webhook
    /// events` to list valid values).
    #[arg(long, value_name = "EVENT", required = true, num_args = 1..)]
    events: Vec<String>,
}

pub(super) fn group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(
        GroupSpec::new("add", "Add components to an application").with_long(
            "Append actions, webhook subscriptions, or UI extensions to the \
            godaddy.toml manifest in the current directory. Run `gddy platform \
            app deploy` to publish the updated manifest.",
        ),
    )
    .with_command(RuntimeCommandSpec::new_typed_with_context::<
        ActionArgs,
        _,
        _,
        _,
    >(
        CommandSpec::from_args::<ActionArgs>("action", "Add an action to godaddy.toml")
            .with_long(
                "Append an action entry to the godaddy.toml manifest in the \
                current directory. An action is an HTTP endpoint that the \
                platform calls on behalf of the application; it is identified \
                by a name and a public HTTPS URL. The manifest is updated in \
                place; run `gddy platform app validate <name>` to confirm remote \
                application state.",
            )
            .with_system("applications")
            .with_tier(Tier::Mutate)
            .with_output_schema::<ConfigAction>()
            .no_auth(true),
        |ctx, args: ActionArgs| async move {
            let name = args.name;
            let url = args.url;
            let path = crate::config::config_path(Some(&ctx.middleware.env));
            let mut config = crate::config::read_config(&path)
                .map_err(|e| cli_engine::CliCoreError::message(e.to_string()))?;
            config.actions.push(crate::config::ActionConfig {
                name: name.clone(),
                url: url.clone(),
            });
            crate::config::write_config(&path, &config)
                .map_err(|e| cli_engine::CliCoreError::message(e.to_string()))?;
            Ok(CommandResult::new(json!({ "name": name, "url": url }))
                .with_next_actions(super::add_config_next_actions(&config.name)))
        },
    ))
    .with_command(RuntimeCommandSpec::new_typed_with_context::<
        SubscriptionArgs,
        _,
        _,
        _,
    >(
        CommandSpec::from_args::<SubscriptionArgs>(
            "subscription",
            "Add a webhook subscription to godaddy.toml",
        )
        .with_long(
            "Append a webhook subscription entry to the godaddy.toml manifest \
            in the current directory. A subscription routes platform events to \
            an HTTPS endpoint. Provide one or more event types with --events; \
            run `gddy platform webhook events` to discover the full list of \
            valid event types.",
        )
        .with_system("applications")
        .with_tier(Tier::Mutate)
        .with_output_schema::<ConfigSubscription>()
        .no_auth(true),
        |ctx, args: SubscriptionArgs| async move {
            let name = args.name;
            let url = args.url;
            let events = args.events;
            let path = crate::config::config_path(Some(&ctx.middleware.env));
            let mut config = crate::config::read_config(&path)
                .map_err(|e| cli_engine::CliCoreError::message(e.to_string()))?;
            let subs = config
                .subscriptions
                .get_or_insert_with(|| crate::config::SubscriptionsConfig { webhook: vec![] });
            subs.webhook.push(crate::config::SubscriptionConfig {
                name: name.clone(),
                events: events.clone(),
                url: url.clone(),
            });
            crate::config::write_config(&path, &config)
                .map_err(|e| cli_engine::CliCoreError::message(e.to_string()))?;
            Ok(
                CommandResult::new(json!({ "name": name, "url": url, "events": events }))
                    .with_next_actions(super::add_config_next_actions(&config.name)),
            )
        },
    ))
    .with_group(super::add_extension::group())
}
