//! `gddy platform app add` — append actions and webhook subscriptions to
//! godaddy.toml. The `extension` subgroup lives in [`super::add_extension`].

use cli_engine::{
    CommandResult, CommandSpec, GroupSpec, RuntimeCommandSpec, RuntimeGroupSpec, Tier,
};
use serde_json::json;

use super::schemas::{ConfigAction, ConfigNativeExtension, ConfigSetting, ConfigSubscription};

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
    /// delete). Defaults to read+write server-side when omitted.
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

#[derive(Debug, Clone, clap::Args)]
struct NativeExtensionArgs {
    /// Display name for the native extension. Falls back to the app `name`
    /// in godaddy.toml at `gddy platform app release` time when omitted.
    #[arg(long)]
    name: Option<String>,

    /// Support contact email written into godaddy.toml as support_contact.
    #[arg(long = "support-contact", value_name = "EMAIL")]
    support_contact: String,

    /// Android package name written into godaddy.toml as android_package_name.
    #[arg(long = "android-package-name", value_name = "PACKAGE")]
    android_package_name: String,
}

fn apply_native_extension(
    config: &mut crate::config::Config,
    name: Option<String>,
    support_contact: String,
    android_package_name: String,
) {
    config.native_extension = Some(crate::config::NativeExtensionConfig {
        name,
        support_contact,
        android_package_name,
    });
}

pub(super) fn group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(
        GroupSpec::new("add", "Add components to an application").with_long(
            "Append actions, webhook subscriptions, UI extensions, or a native \
            extension to the godaddy.toml manifest in the current directory. Run \
            `gddy platform app deploy` to publish the updated manifest.",
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
            — it cannot author the settings-form-v1 form itself. After running \
            it, hand-add a [settings.presentation] block (sections and fields) \
            to the written entry; `gddy platform app release` rejects a \
            settings entry with no presentation.",
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
    .with_command(RuntimeCommandSpec::new_typed_with_context::<
        NativeExtensionArgs,
        _,
        _,
        _,
    >(
        CommandSpec::from_args::<NativeExtensionArgs>(
            "native-extension",
            "Add a native Android extension to godaddy.toml",
        )
        .with_long(
            "Write a [native_extension] section to the godaddy.toml manifest in \
            the current directory. support_contact and android_package_name are \
            required; name is optional and falls back to the app name at \
            `gddy platform app release` time. This command only edits the local \
            manifest — the native-app draft is created when you run \
            `gddy platform app release`. Re-running this command overwrites the \
            existing [native_extension] section.",
        )
        .with_system("applications")
        .with_tier(Tier::Mutate)
        .with_output_schema::<ConfigNativeExtension>()
        .no_auth(true),
        |ctx, args: NativeExtensionArgs| async move {
            let name = args.name;
            let support_contact = args.support_contact;
            let android_package_name = args.android_package_name;
            let path = crate::config::config_path(Some(&ctx.middleware.env));
            let mut config = crate::config::read_config(&path)
                .map_err(|e| crate::error::GddyError::config(e.to_string()).into_cli_error())?;
            apply_native_extension(
                &mut config,
                name.clone(),
                support_contact.clone(),
                android_package_name.clone(),
            );
            crate::config::write_config(&path, &config)
                .map_err(|e| crate::error::GddyError::config(e.to_string()).into_cli_error())?;
            Ok(CommandResult::new(json!({
                "name": name,
                "supportContact": support_contact,
                "androidPackageName": android_package_name,
            }))
            .with_next_actions(super::add_config_next_actions(&config.name)))
        },
    ))
    .with_group(super::add_extension::group())
}

#[cfg(test)]
mod tests {
    #[test]
    fn native_extension_subcommand_accepts_required_and_optional_flags() {
        super::group()
            .clap_command()
            .try_get_matches_from([
                "add",
                "native-extension",
                "--name",
                "My Display Name",
                "--support-contact",
                "support@example.com",
                "--android-package-name",
                "com.example.app",
            ])
            .expect("native-extension flags should be accepted");
    }

    #[test]
    fn native_extension_subcommand_name_is_optional() {
        super::group()
            .clap_command()
            .try_get_matches_from([
                "add",
                "native-extension",
                "--support-contact",
                "support@example.com",
                "--android-package-name",
                "com.example.app",
            ])
            .expect("--name is optional");
    }

    #[test]
    fn native_extension_subcommand_requires_support_contact() {
        let err = super::group()
            .clap_command()
            .try_get_matches_from([
                "add",
                "native-extension",
                "--android-package-name",
                "com.example.app",
            ])
            .expect_err("--support-contact is required");
        let msg = err.to_string();
        assert!(
            msg.contains("support-contact"),
            "unexpected clap error: {msg}"
        );
    }

    #[test]
    fn native_extension_subcommand_requires_android_package_name() {
        let err = super::group()
            .clap_command()
            .try_get_matches_from([
                "add",
                "native-extension",
                "--support-contact",
                "support@example.com",
            ])
            .expect_err("--android-package-name is required");
        let msg = err.to_string();
        assert!(
            msg.contains("android-package-name"),
            "unexpected clap error: {msg}"
        );
    }

    #[test]
    fn apply_native_extension_overwrites_and_round_trips_through_toml() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("godaddy.toml");
        let mut config = {
            // Mirror config::tests::valid_config field-for-field so this test
            // does not depend on that helper (it is private to config/mod.rs).
            crate::config::Config {
                name: "my-app".to_owned(),
                client_id: "550e8400-e29b-41d4-a716-446655440000".to_owned(),
                description: Some("test".to_owned()),
                version: "1.2.3".to_owned(),
                url: "https://example.com".to_owned(),
                proxy_url: "https://proxy.example.com".to_owned(),
                authorization_scopes: vec!["openid".to_owned()],
                actions: vec![],
                subscriptions: None,
                dependencies: vec![],
                extensions: None,
                settings: vec![],
                native_extension: None,
            }
        };
        crate::config::write_config(&path, &config).expect("write base");

        super::apply_native_extension(
            &mut config,
            Some("My Display Name".to_owned()),
            "support@example.com".to_owned(),
            "com.example.app".to_owned(),
        );
        crate::config::write_config(&path, &config).expect("write native_extension");

        let read_back = crate::config::read_config(&path).expect("read back");
        let native = read_back
            .native_extension
            .expect("native_extension section written");
        assert_eq!(native.name.as_deref(), Some("My Display Name"));
        assert_eq!(native.support_contact, "support@example.com");
        assert_eq!(native.android_package_name, "com.example.app");

        super::apply_native_extension(
            &mut config,
            None,
            "other@example.com".to_owned(),
            "com.example.other".to_owned(),
        );
        crate::config::write_config(&path, &config).expect("overwrite");
        let overwritten = crate::config::read_config(&path).expect("read overwrite");
        let native = overwritten.native_extension.expect("still present");
        assert_eq!(native.name, None);
        assert_eq!(native.support_contact, "other@example.com");
        assert_eq!(native.android_package_name, "com.example.other");
    }

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
