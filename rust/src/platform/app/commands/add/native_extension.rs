//! `gddy platform app add native-extension` — synchronize a native Android app record.

use cli_engine::{CommandResult, CommandSpec, RuntimeCommandSpec, Stage, Tier};
use serde_json::json;

use super::super::schemas::ConfigNativeExtension;
use crate::platform::app::native_app_client::{NativeAppClient, NativeAppInput, UpsertOperation};
use crate::scopes::{APP_REGISTRY_READ, APP_REGISTRY_WRITE};

#[derive(Debug, Clone, clap::Args)]
pub(super) struct NativeExtensionArgs {
    /// Display name for the native extension. Falls back to the app `name`
    /// in godaddy.toml when omitted.
    #[arg(long)]
    pub(super) name: Option<String>,

    /// Support contact email written into godaddy.toml as support_contact.
    #[arg(long = "support-contact", value_name = "EMAIL")]
    pub(super) support_contact: String,

    /// Android package name written into godaddy.toml as android_package_name.
    #[arg(long = "android-package-name", value_name = "PACKAGE")]
    pub(super) android_package_name: String,

    /// Accept GoDaddy Developer agreements non-interactively when onboarding
    /// is still pending (required for non-TTY). Used only when creating a
    /// native-app record.
    #[arg(long)]
    pub(super) accept_agreements: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct NativeExtensionRegistration {
    pub(super) application_id: String,
    pub(super) operation: UpsertOperation,
}

/// Native-app display name: `[native_extension].name` when present and
/// non-empty, otherwise the application name.
fn native_app_name(config: &crate::config::Config) -> &str {
    config
        .native_extension
        .as_ref()
        .and_then(|native| native.name.as_deref())
        .filter(|name| !name.is_empty())
        .unwrap_or(&config.name)
}

pub(super) fn apply_native_extension(
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

pub(super) fn prepare_native_extension(
    path: &std::path::Path,
    args: &NativeExtensionArgs,
) -> cli_engine::Result<crate::config::Config> {
    let mut config = crate::config::read_config(path)
        .map_err(|error| crate::error::GddyError::config(error.to_string()).into_cli_error())?;
    apply_native_extension(
        &mut config,
        args.name.clone(),
        args.support_contact.clone(),
        args.android_package_name.clone(),
    );
    config
        .validate()
        .map_err(|error| crate::error::GddyError::validation(error.to_string()).into_cli_error())?;
    // Serialize before any remote work so an invalid TOML shape cannot leave
    // DevX Core ahead of the local manifest.
    toml::to_string_pretty(&config)
        .map_err(|error| crate::error::GddyError::config(error.to_string()).into_cli_error())?;
    Ok(config)
}

pub(super) fn application_id(data: &serde_json::Value, name: &str) -> cli_engine::Result<String> {
    let application = &data["application"];
    if application.is_null() {
        return Err(crate::error::GddyError::not_found(format!(
            "application {name:?} was not found"
        ))
        .with_system("applications")
        .into_cli_error());
    }

    application["id"]
        .as_str()
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| {
            crate::error::GddyError::unexpected(format!(
                "App Registry returned application {name:?} without an id"
            ))
            .with_system("applications")
            .into_cli_error()
        })
}

pub(super) async fn sync_native_extension(
    config: &crate::config::Config,
    token: &str,
    app_registry_url: &str,
    devx_core_url: &str,
    accept_agreements: bool,
) -> cli_engine::Result<NativeExtensionRegistration> {
    let app_registry =
        crate::platform::app::client::ApplicationClient::new(app_registry_url, token.to_owned());
    let application = app_registry
        .get_application(&config.name)
        .await
        .map_err(super::super::client_err)?;
    let application_id = application_id(&application, &config.name)?;

    let native = config
        .native_extension
        .as_ref()
        .expect("native extension is installed during preparation");
    let input = NativeAppInput {
        name: native_app_name(config).to_owned(),
        description: config.description.clone().unwrap_or_default(),
        support_email: native.support_contact.clone(),
        app_category: String::new(),
        merchant_category: String::new(),
        android_package_name: native.android_package_name.clone(),
        status: "draft".to_owned(),
    };
    // Only a create needs the agreement gate. An update never calls onboarding,
    // and the PATCH carries only the fields this command owns.
    let operation = NativeAppClient::new(devx_core_url, token)
        .upsert(&application_id, &input, || async {
            crate::platform::app::onboarding::ensure_ready_for_app_init_at(
                token,
                devx_core_url,
                accept_agreements,
            )
            .await
            .map(|outcome| outcome.org_id)
        })
        .await?;

    Ok(NativeExtensionRegistration {
        application_id,
        operation,
    })
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<NativeExtensionArgs, _, _, _>(
        CommandSpec::from_args::<NativeExtensionArgs>(
            "native-extension",
            "Register a native Android extension",
        )
        .with_long(
            "Write a [native_extension] section to the godaddy.toml manifest in \
            the current directory and immediately create or update its DevX Core \
            native-app record. The command authenticates, looks up the App Registry \
            application by the manifest's name, and uses that application's ID. \
            support_contact and android_package_name are required; name is optional \
            and falls back to the application name. The local file is written only \
            after the remote record succeeds, and rerunning safely reconciles either \
            an existing remote record or an existing local section. The record is \
            requested as a draft, but DevX Core may currently store it as active. \
            Creating a record requires accepted developer agreements; a non-interactive \
            session passes --accept-agreements when onboarding is still pending. \
            Updating an existing record leaves portal-owned categories and description \
            unchanged.",
        )
        .with_system("applications")
        .with_tier(Tier::Mutate)
        .with_scopes(&[APP_REGISTRY_READ, APP_REGISTRY_WRITE])
        .with_output_schema::<ConfigNativeExtension>()
        .with_feature_flag(super::super::NATIVE_APPS_FLAG_KEY, Stage::Experimental),
        |ctx, args: NativeExtensionArgs| async move {
            let path = crate::config::config_path(Some(&ctx.middleware.env));
            let config = prepare_native_extension(&path, &args)?;
            let app_registry_url = crate::http::api_url_for_env(&ctx.middleware.env)?;
            let devx_core_url = crate::environments::devx_core_url(&ctx.middleware.env)
                .ok_or_else(|| {
                    crate::error::GddyError::config(format!(
                        "DevX Core URL is not configured for environment {:?}",
                        ctx.middleware.env
                    ))
                    .into_cli_error()
                })?;
            // Resolve credentials only after all local validation has passed.
            let token = ctx.credential().await?.token;
            let registration = sync_native_extension(
                &config,
                &token,
                &app_registry_url,
                &devx_core_url,
                args.accept_agreements,
            )
            .await?;
            crate::config::write_config(&path, &config).map_err(|error| {
                crate::error::GddyError::config(format!(
                    "The DevX Core native-app record was {} for application {}, but {} could not be updated: {error}",
                    registration.operation.as_str(),
                    registration.application_id,
                    path.display(),
                ))
                .with_fix("Rerun this idempotent command to reconcile the local manifest with the existing remote record.")
                .into_cli_error()
            })?;
            let native = config
                .native_extension
                .as_ref()
                .expect("native extension is installed during preparation");
            Ok(CommandResult::new(json!({
                "applicationId": registration.application_id,
                "operation": registration.operation.as_str(),
                "name": native_app_name(&config),
                "supportContact": native.support_contact,
                "androidPackageName": native.android_package_name,
            }))
            .with_next_actions(super::super::add_config_next_actions(&config.name)))
        },
    )
}

#[cfg(test)]
mod tests {
    use httpmock::{Method, MockServer};
    use serde_json::json;

    use super::*;

    fn test_config() -> crate::config::Config {
        crate::config::Config {
            name: "my-app".to_owned(),
            client_id: "550e8400-e29b-41d4-a716-446655440000".to_owned(),
            description: Some("test".to_owned()),
            version: "1.2.3".to_owned(),
            url: "https://example.com".to_owned(),
            proxy_url: "https://proxy.example.com".to_owned(),
            authorization_scopes: vec!["openid".to_owned()],
            redirect_uris: None,
            actions: vec![],
            subscriptions: None,
            dependencies: vec![],
            extensions: None,
            settings: vec![],
            native_extension: None,
        }
    }

    fn native_args(support_contact: &str) -> NativeExtensionArgs {
        NativeExtensionArgs {
            name: Some("My Display Name".to_owned()),
            support_contact: support_contact.to_owned(),
            android_package_name: "com.example.app".to_owned(),
            accept_agreements: false,
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
    fn native_app_name_falls_back_to_application_name_when_absent_or_empty() {
        let mut config = test_config();
        for name in [None, Some(String::new())] {
            apply_native_extension(
                &mut config,
                name,
                "support@example.com".to_owned(),
                "com.example.app".to_owned(),
            );
            assert_eq!(native_app_name(&config), "my-app");
        }
        apply_native_extension(
            &mut config,
            Some("My Display Name".to_owned()),
            "support@example.com".to_owned(),
            "com.example.app".to_owned(),
        );
        assert_eq!(native_app_name(&config), "My Display Name");
    }

    #[test]
    fn apply_overwrites_and_round_trips_through_toml() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("godaddy.toml");
        let mut config = test_config();
        crate::config::write_config(&path, &config).expect("write base");
        apply_native_extension(
            &mut config,
            Some("My Display Name".to_owned()),
            "support@example.com".to_owned(),
            "com.example.app".to_owned(),
        );
        crate::config::write_config(&path, &config).expect("write native extension");

        let native = crate::config::read_config(&path)
            .expect("read back")
            .native_extension
            .expect("native extension");
        assert_eq!(native.name.as_deref(), Some("My Display Name"));
        assert_eq!(native.support_contact, "support@example.com");
        assert_eq!(native.android_package_name, "com.example.app");
    }

    #[test]
    fn invalid_support_email_is_rejected_during_local_preparation() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("godaddy.toml");
        crate::config::write_config(&path, &test_config()).expect("write base config");

        let error = prepare_native_extension(&path, &native_args("not-an-email"))
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
            application_id(&data, "my-app").expect("application id"),
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
                    "data": { "application": { "id": "app-registry-id", "name": "my-app" } }
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
        apply_native_extension(
            &mut config,
            Some("My Display Name".to_owned()),
            "support@example.com".to_owned(),
            "com.example.app".to_owned(),
        );

        let registration = sync_native_extension(
            &config,
            "test-token",
            &app_registry.base_url(),
            &devx_core.base_url(),
            false,
        )
        .await
        .expect("sync native extension");

        assert_eq!(registration.application_id, "app-registry-id");
        assert_eq!(registration.operation, UpsertOperation::Created);
        app_lookup.assert_async().await;
        onboarding.assert_async().await;
        get.assert_async().await;
        create.assert_async().await;
        support_patch.assert_async().await;
    }

    #[tokio::test]
    async fn sync_updates_existing_record_without_calling_onboarding() {
        let app_registry = MockServer::start_async().await;
        app_registry
            .mock_async(|when, then| {
                when.method(Method::POST)
                    .path("/v1/apps/app-registry-subgraph");
                then.status(200).json_body(json!({
                    "data": { "application": { "id": "app-registry-id", "name": "my-app" } }
                }));
            })
            .await;
        let devx_core = MockServer::start_async().await;
        // Onboarding is unavailable; an update must not depend on it.
        let onboarding = devx_core
            .mock_async(|when, then| {
                when.method(Method::POST).path("/api/v1/onboarding/status");
                then.status(503);
            })
            .await;
        let get = devx_core
            .mock_async(|when, then| {
                when.method(Method::GET)
                    .path("/api/v1/native-apps/app-registry-id");
                then.status(200).json_body(json!({
                    "success": true,
                    "data": native_app_json()
                }));
            })
            .await;
        let update = devx_core
            .mock_async(|when, then| {
                when.method(Method::PATCH)
                    .path("/api/v1/native-apps/app-registry-id")
                    .json_body(json!({
                        "name": "My Display Name",
                        "supportEmail": "support@example.com",
                        "androidPackageName": "com.example.app"
                    }));
                then.status(200).json_body(json!({
                    "success": true,
                    "data": native_app_json()
                }));
            })
            .await;
        let mut config = test_config();
        apply_native_extension(
            &mut config,
            Some("My Display Name".to_owned()),
            "support@example.com".to_owned(),
            "com.example.app".to_owned(),
        );

        let registration = sync_native_extension(
            &config,
            "test-token",
            &app_registry.base_url(),
            &devx_core.base_url(),
            false,
        )
        .await
        .expect("update should not need onboarding");

        assert_eq!(registration.operation, UpsertOperation::Updated);
        get.assert_async().await;
        update.assert_async().await;
        assert_eq!(onboarding.calls_async().await, 0);
    }

    #[tokio::test]
    async fn sync_create_requires_accepted_agreements_when_onboarding_is_pending() {
        let app_registry = MockServer::start_async().await;
        app_registry
            .mock_async(|when, then| {
                when.method(Method::POST)
                    .path("/v1/apps/app-registry-subgraph");
                then.status(200).json_body(json!({
                    "data": { "application": { "id": "app-registry-id", "name": "my-app" } }
                }));
            })
            .await;
        let devx_core = MockServer::start_async().await;
        let onboarding = devx_core
            .mock_async(|when, then| {
                when.method(Method::POST).path("/api/v1/onboarding/status");
                then.status(200).json_body(json!({
                    "success": true,
                    "data": {
                        "id": "550e8400-e29b-41d4-a716-446655440001",
                        "status": "PENDING"
                    }
                }));
            })
            .await;
        let complete = devx_core
            .mock_async(|when, then| {
                when.method(Method::POST).path("/api/v1/onboarding/cli");
                then.status(200).json_body(json!({
                    "success": true,
                    "data": {
                        "organizationId": "550e8400-e29b-41d4-a716-446655440001",
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
                    .path("/api/v1/native-apps/app-registry-id");
                then.status(200).json_body(json!({
                    "success": true,
                    "data": native_app_json()
                }));
            })
            .await;
        let mut config = test_config();
        apply_native_extension(
            &mut config,
            Some("My Display Name".to_owned()),
            "support@example.com".to_owned(),
            "com.example.app".to_owned(),
        );

        let error = sync_native_extension(
            &config,
            "test-token",
            &app_registry.base_url(),
            &devx_core.base_url(),
            false,
        )
        .await
        .expect_err("pending onboarding must block create");

        let envelope = cli_engine::build_error_envelope(&error, "applications");
        assert_eq!(
            envelope.error.as_ref().map(|item| item.code.as_str()),
            Some("AGREEMENTS_REQUIRED")
        );
        assert!(
            error.to_string().contains("agreements must be accepted"),
            "{error}"
        );
        get.assert_async().await;
        onboarding.assert_async().await;
        assert_eq!(complete.calls_async().await, 0);
        assert_eq!(create.calls_async().await, 0);
    }

    #[tokio::test]
    async fn sync_create_completes_pending_onboarding_when_agreements_are_accepted() {
        let app_registry = MockServer::start_async().await;
        app_registry
            .mock_async(|when, then| {
                when.method(Method::POST)
                    .path("/v1/apps/app-registry-subgraph");
                then.status(200).json_body(json!({
                    "data": { "application": { "id": "app-registry-id", "name": "my-app" } }
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
                        "status": "PENDING"
                    }
                }));
            })
            .await;
        let complete = devx_core
            .mock_async(|when, then| {
                when.method(Method::POST).path("/api/v1/onboarding/cli");
                then.status(200).json_body(json!({
                    "success": true,
                    "data": {
                        "organizationId": "550e8400-e29b-41d4-a716-446655440001",
                        "status": "ACTIVE"
                    }
                }));
            })
            .await;
        devx_core
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
        devx_core
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
        apply_native_extension(
            &mut config,
            Some("My Display Name".to_owned()),
            "support@example.com".to_owned(),
            "com.example.app".to_owned(),
        );

        let registration = sync_native_extension(
            &config,
            "test-token",
            &app_registry.base_url(),
            &devx_core.base_url(),
            true,
        )
        .await
        .expect("accepted agreements should allow create");

        assert_eq!(registration.operation, UpsertOperation::Created);
        complete.assert_async().await;
        create.assert_async().await;
    }

    #[tokio::test]
    async fn remote_failure_leaves_local_manifest_unchanged() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("godaddy.toml");
        crate::config::write_config(&path, &test_config()).expect("write base config");
        let prepared = prepare_native_extension(&path, &native_args("support@example.com"))
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

        let error = sync_native_extension(
            &prepared,
            "test-token",
            &app_registry.base_url(),
            &devx_core.base_url(),
            false,
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
}
