//! `gddy platform app add` — append actions and webhook subscriptions to
//! godaddy.toml. The `extension` subgroup lives in [`super::add_extension`].

use cli_engine::{
    CommandResult, CommandSpec, GroupSpec, RuntimeCommandSpec, RuntimeGroupSpec, Tier,
};
use serde_json::json;

use super::schemas::{ConfigAction, ConfigSetting, ConfigSubscription};

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
struct SettingsArgs {
    /// Commerce-owned settings group slug written into godaddy.toml.
    #[arg(long)]
    group: String,

    /// App-owned setting slug written into godaddy.toml.
    #[arg(long)]
    slug: String,

    /// Display title for the settings entry.
    #[arg(long)]
    title: Option<String>,

    /// Display description for the settings entry.
    #[arg(long)]
    description: Option<String>,

    /// GPA settings namespace path lifecycle endpoints live beneath.
    #[arg(long = "entry-path", value_name = "PATH")]
    entry_path: String,

    /// Sort order within the settings group.
    #[arg(long)]
    order: Option<i64>,

    /// One or more lifecycle capabilities (read, write, validate, test,
    /// delete, open) — a link presentation requires exactly read+open.
    #[arg(long = "capability", value_name = "CAPABILITY", num_args = 1..)]
    capabilities: Vec<String>,

    /// Icon name for display; must be provided together with --icon-library.
    #[arg(long = "icon-name", value_name = "NAME")]
    icon_name: Option<String>,

    /// Icon library for display; must be provided together with --icon-name.
    #[arg(long = "icon-library", value_name = "LIBRARY")]
    icon_library: Option<String>,

    /// Path to a JSON presentation file; alternative to hand-authoring
    /// [settings.presentation].
    #[arg(long = "presentation-file", value_name = "PATH")]
    presentation_file: Option<String>,
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
    .with_command(RuntimeCommandSpec::new_typed_with_context::<
        SettingsArgs,
        _,
        _,
        _,
    >(
        CommandSpec::from_args::<SettingsArgs>(
            "settings",
            "Add an application settings placement to godaddy.toml",
        )
        .with_long(
            "Register the placement metadata for an application-settings \
            capability in the godaddy.toml manifest in the current directory. \
            This command only writes group/slug/entryPath/order/capabilities/icon \
            — it cannot author the settings-form-v1 form or settings-link-v1 \
            link itself. After running it, hand-add a [settings.presentation] \
            block to the written entry — sections and fields for a form, or a \
            label and openMode for a link; `gddy platform app release` rejects \
            a settings entry with no presentation.",
        )
        .with_system("applications")
        .with_tier(Tier::Mutate)
        .with_output_schema::<ConfigSetting>()
        .no_auth(true),
        |ctx, args: SettingsArgs| async move {
            let group = args.group;
            let slug = args.slug;
            let entry_path = args.entry_path;
            if args.icon_name.is_some() != args.icon_library.is_some() {
                return Err(crate::error::GddyError::validation(
                    "--icon-name and --icon-library must be provided together",
                )
                .into_cli_error());
            }
            let icon = args
                .icon_name
                .zip(args.icon_library)
                .map(|(name, library)| crate::config::SettingIcon { name, library });
            let path = crate::config::config_path(Some(&ctx.middleware.env));
            let mut config = crate::config::read_config(&path)
                .map_err(|e| crate::error::GddyError::config(e.to_string()).into_cli_error())?;
            config.settings.push(crate::config::SettingConfig {
                group: group.clone(),
                slug: slug.clone(),
                title: args.title,
                description: args.description,
                entry_path: entry_path.clone(),
                order: args.order,
                capabilities: args.capabilities,
                icon,
                metadata: None,
                presentation_file: args.presentation_file,
                presentation: None,
            });
            crate::config::write_config(&path, &config)
                .map_err(|e| crate::error::GddyError::config(e.to_string()).into_cli_error())?;
            Ok(
                CommandResult::new(
                    json!({ "group": group, "slug": slug, "entryPath": entry_path }),
                )
                .with_next_actions(super::add_config_next_actions(&config.name)),
            )
        },
    ))
    .with_group(super::add_extension::group())
}

#[cfg(test)]
mod tests {
    #[test]
    fn settings_subcommand_accepts_presentation_file_flag() {
        super::group()
            .clap_command()
            .try_get_matches_from([
                "add",
                "settings",
                "--group",
                "tax-center",
                "--slug",
                "godaddy-tax",
                "--entry-path",
                "/settings/godaddy-tax",
                "--presentation-file",
                "fixtures/manual-tax-presentation.json",
            ])
            .expect("--presentation-file flag should be accepted");
    }
}
