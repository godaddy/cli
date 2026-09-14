//! `gddy platform app add` — append actions and webhook subscriptions to
//! godaddy.toml. The `extension` subgroup lives in [`super::add_extension`].

use cli_engine::{
    CommandResult, CommandSpec, GroupSpec, RuntimeCommandSpec, RuntimeGroupSpec, Tier,
};
use serde_json::json;

use super::schemas::{ConfigAction, ConfigSetting, ConfigSubscription};

mod native_extension;

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
            "Add actions, webhook subscriptions, UI extensions, or a native \
            extension to the godaddy.toml manifest in the current directory. \
            Native extensions are also registered immediately as DevX Core \
            drafts; other components are published by a later deploy or release.",
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
    .with_command(native_extension::command())
    .with_group(super::add_extension::group())
}

#[cfg(test)]
mod tests {
    use httpmock::{Method, MockServer};
    use serde_json::json;

    fn test_config() -> crate::config::Config {
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
    }

    fn native_args(support_contact: &str) -> super::native_extension::NativeExtensionArgs {
        super::native_extension::NativeExtensionArgs {
            name: Some("My Display Name".to_owned()),
            support_contact: support_contact.to_owned(),
            android_package_name: "com.example.app".to_owned(),
        }
    }

    fn native_app_json() -> serde_json::Value {
        json!({
            "applicationId": "app-registry-id",
            "name": "My Display Name",
            "description": "test",
            "supportEmail": "support@example.com",
            "appCategory": "",
            "merchantCategory": "",
            "androidPackageName": "com.example.app",
            "status": "draft",
            "released": false
        })
    }

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
        let mut config = test_config();
        crate::config::write_config(&path, &config).expect("write base");

        super::native_extension::apply_native_extension(
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

        super::native_extension::apply_native_extension(
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
    fn invalid_support_email_is_rejected_during_local_preparation() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("godaddy.toml");
        crate::config::write_config(&path, &test_config()).expect("write base config");

        let error =
            super::native_extension::prepare_native_extension(&path, &native_args("not-an-email"))
                .expect_err("invalid support email must fail");

        assert!(error.to_string().contains("valid email address"), "{error}");
        assert!(
            crate::config::read_config(&path)
                .expect("read unchanged config")
                .native_extension
                .is_none()
        );
    }

    #[test]
    fn application_name_lookup_uses_registry_id_not_oauth_client_id() {
        let data = json!({
            "application": {
                "id": "app-registry-id",
                "clientId": "550e8400-e29b-41d4-a716-446655440000",
                "name": "my-app"
            }
        });
        assert_eq!(
            super::native_extension::application_id(&data, "my-app").expect("application id"),
            "app-registry-id"
        );
    }

    #[tokio::test]
    async fn sync_resolves_application_and_organization_then_creates_draft() {
        let app_registry = MockServer::start_async().await;
        let app_lookup = app_registry
            .mock_async(|when, then| {
                when.method(Method::POST)
                    .path("/v1/apps/app-registry-subgraph")
                    .header("authorization", "Bearer test-token")
                    .is_true(|request| request.body_string().contains(r#""name":"my-app""#));
                then.status(200).json_body(json!({
                    "data": {
                        "application": {
                            "id": "app-registry-id",
                            "name": "my-app"
                        }
                    }
                }));
            })
            .await;
        let devx_core = MockServer::start_async().await;
        let onboarding = devx_core
            .mock_async(|when, then| {
                when.method(Method::POST)
                    .path("/api/v1/onboarding/status")
                    .header("authorization", "Bearer test-token");
                then.status(200).json_body(json!({
                    "success": true,
                    "data": {
                        "id": "550e8400-e29b-41d4-a716-446655440001",
                        "status": "ACTIVE"
                    }
                }));
            })
            .await;
        let get = devx_core
            .mock_async(|when, then| {
                when.method(Method::GET)
                    .path("/api/v1/native-apps/app-registry-id");
                then.status(200)
                    .json_body(json!({ "success": true, "data": null }));
            })
            .await;
        let create = devx_core
            .mock_async(|when, then| {
                when.method(Method::POST)
                    .path("/api/v1/native-apps/app-registry-id")
                    .json_body(json!({
                        "organizationId": "550e8400-e29b-41d4-a716-446655440001",
                        "name": "My Display Name",
                        "description": "test",
                        "supportEmail": "support@example.com",
                        "appCategory": "",
                        "merchantCategory": "",
                        "androidPackageName": "com.example.app",
                        "status": "draft"
                    }));
                then.status(200).json_body(json!({
                    "success": true,
                    "data": native_app_json()
                }));
            })
            .await;
        let support_patch = devx_core
            .mock_async(|when, then| {
                when.method(Method::PATCH)
                    .path("/api/v1/native-apps/app-registry-id")
                    .json_body(json!({ "supportEmail": "support@example.com" }));
                then.status(200).json_body(json!({
                    "success": true,
                    "data": native_app_json()
                }));
            })
            .await;
        let mut config = test_config();
        super::native_extension::apply_native_extension(
            &mut config,
            Some("My Display Name".to_owned()),
            "support@example.com".to_owned(),
            "com.example.app".to_owned(),
        );

        let registration = super::native_extension::sync_native_extension(
            &config,
            "test-token",
            &app_registry.base_url(),
            &devx_core.base_url(),
        )
        .await
        .expect("sync native extension");

        assert_eq!(registration.application_id, "app-registry-id");
        assert_eq!(
            registration.operation,
            crate::application::native_app_client::UpsertOperation::Created
        );
        app_lookup.assert_async().await;
        onboarding.assert_async().await;
        get.assert_async().await;
        create.assert_async().await;
        support_patch.assert_async().await;
    }

    #[tokio::test]
    async fn remote_failure_leaves_local_manifest_unchanged() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("godaddy.toml");
        crate::config::write_config(&path, &test_config()).expect("write base config");
        let prepared = super::native_extension::prepare_native_extension(
            &path,
            &native_args("support@example.com"),
        )
        .expect("prepare native extension");

        let app_registry = MockServer::start_async().await;
        app_registry
            .mock_async(|when, then| {
                when.method(Method::POST)
                    .path("/v1/apps/app-registry-subgraph");
                then.status(200).json_body(json!({
                    "data": { "application": { "id": "app-registry-id" } }
                }));
            })
            .await;
        let devx_core = MockServer::start_async().await;
        devx_core
            .mock_async(|when, then| {
                when.method(Method::POST).path("/api/v1/onboarding/status");
                then.status(200).json_body(json!({
                    "success": true,
                    "data": {
                        "id": "550e8400-e29b-41d4-a716-446655440001",
                        "status": "ACTIVE"
                    }
                }));
            })
            .await;
        devx_core
            .mock_async(|when, then| {
                when.method(Method::GET)
                    .path("/api/v1/native-apps/app-registry-id");
                then.status(200).json_body(json!({
                    "success": true,
                    "data": native_app_json()
                }));
            })
            .await;
        devx_core
            .mock_async(|when, then| {
                when.method(Method::PATCH)
                    .path("/api/v1/native-apps/app-registry-id");
                then.status(409).json_body(json!({
                    "success": false,
                    "error": {
                        "code": "PACKAGE_NAME_IMMUTABLE",
                        "message": "Package name cannot change after release"
                    }
                }));
            })
            .await;

        let error = super::native_extension::sync_native_extension(
            &prepared,
            "test-token",
            &app_registry.base_url(),
            &devx_core.base_url(),
        )
        .await
        .expect_err("remote update must fail");

        assert!(
            error.to_string().contains("PACKAGE_NAME_IMMUTABLE"),
            "{error}"
        );
        assert!(
            crate::config::read_config(&path)
                .expect("read original manifest")
                .native_extension
                .is_none()
        );
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
