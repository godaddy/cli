//! `gddy platform app deploy` — bundle, scan, and upload an application's
//! extensions, streaming progress as JSON events. Extension-level bundling
//! and upload logic lives in [`extensions`].

use cli_engine::{
    CommandSpec, NextAction, NextActionParam, RuntimeCommandSpec, StreamSender, Tier,
};
use serde_json::{Value, json};

use extensions::DeployExtensionArgs;

use crate::application::client::{ApplicationClient, api_url_for_env};
use crate::next_action::{next_action, required_value};
use crate::scopes::{APP_REGISTRY_READ, APP_REGISTRY_WRITE};

mod extensions;

/// Builds the terminal `{"type":"error",...}` event for a streaming command,
/// reusing `cli_engine::build_error_envelope` so the `code`/`message`/`fix`
/// match what a non-streaming command would have rendered for the same error.
fn deploy_error_event(err: &cli_engine::CliCoreError) -> Value {
    let envelope = cli_engine::build_error_envelope(err, "applications");
    // `build_error_envelope` always populates `.error` with a non-empty code;
    // this fallback only guards against a future change to that guarantee.
    let (code, message) = envelope
        .error
        .map(|e| (e.code, e.message))
        .unwrap_or_else(|| ("ERROR".to_owned(), err.to_string()));
    let mut event = json!({
        "type": "error",
        "ok": false,
        "error": { "code": code, "message": message },
        "next_actions": [],
    });
    if let Some(fix) = envelope.fix.filter(|f| !f.is_empty()) {
        event["fix"] = json!(fix);
    }
    event
}

/// Emit the terminal error event on a streaming command, then return the
/// error so the handler can still fail the run via `?`.
async fn fail_deploy(
    sender: &StreamSender,
    err: cli_engine::CliCoreError,
) -> cli_engine::CliCoreError {
    sender.send(deploy_error_event(&err)).await;
    err
}

/// Next-actions after a successful deploy.
pub(super) fn deploy_next_actions(name: &str) -> Vec<NextAction> {
    vec![
        next_action(
            "platform app enable <name> --store-id <store-id>",
            "Enable the application on a store",
        )
        .with_param("name", required_value(name))
        .with_param("store-id", NextActionParam::required()),
        next_action(
            "platform app info --name <name>",
            "Inspect deployment status",
        )
        .with_param("name", required_value(name)),
        next_action("platform app deploy --name <name>", "Rerun deployment")
            .with_param("name", required_value(name)),
    ]
}

/// Builds the terminal `{"type":"result",...}` event for a successful
/// `platform app deploy` run.
fn deploy_result_event(
    name: &str,
    application_id: &str,
    release_id: &str,
    extensions: usize,
) -> Value {
    json!({
        "type": "result",
        "ok": true,
        "result": {
            "application": name,
            "applicationId": application_id,
            "releaseId": release_id,
            "extensions": extensions,
            "releaseStatus": "ACTIVE",
            "applicationStatus": "ACTIVE",
        },
        "next_actions": deploy_next_actions(name),
    })
}

/// Wrap a fallible step: on `Err`, emit the terminal error event before
/// propagating, so every failure path produces exactly one terminal line.
async fn tap_deploy_err<T>(
    sender: &StreamSender,
    result: cli_engine::Result<T>,
) -> cli_engine::Result<T> {
    match result {
        Ok(v) => Ok(v),
        Err(e) => Err(fail_deploy(sender, e).await),
    }
}

#[derive(Debug, Clone, clap::Args)]
struct DeployArgs {
    /// Application name.
    #[arg(long, short = 'n', value_name = "NAME")]
    name: String,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_streaming::<DeployArgs, _, _>(
        CommandSpec::from_args::<DeployArgs>(
            "deploy",
            "Deploy an application, streaming progress events",
        )
        .with_long(
            "Read godaddy.toml from the current directory, bundle all declared \
            extensions with esbuild, run the pre-bundle security scanner \
            (SEC001–SEC010/SEC012 AST + SEC011 package scripts) and the \
            post-bundle regex scanner (SEC101–SEC115) on each extension, then \
            upload the artifacts to the latest release of the named \
            application and activate both the release and application. An \
            already-active application skips the lifecycle mutation. Progress \
            is streamed as JSON events. A release must exist before deploying; \
            create one with \
            `gddy platform app release`.",
        )
        .with_system("applications")
        .with_tier(Tier::Mutate)
        .with_scopes(&[APP_REGISTRY_READ, APP_REGISTRY_WRITE]),
        |ctx, args: DeployArgs, sender: StreamSender| async move {
            let name = args.name;
            let env = ctx.middleware.env.clone();
            let token = tap_deploy_err(&sender, ctx.credential().await).await?.token;
            let base_url = tap_deploy_err(&sender, api_url_for_env(&env)).await?;
            let client = ApplicationClient::new(base_url, token);

            sender
                .send(json!({ "type": "step", "name": "auth.check", "status": "completed" }))
                .await;

            // Read the local godaddy.toml
            sender
                .send(json!({ "type": "step", "name": "config.read", "status": "started" }))
                .await;
            let config_path = crate::config::config_path(Some(&env));
            let config = tap_deploy_err(
                &sender,
                crate::config::read_config(&config_path).map_err(|e| {
                    crate::error::GddyError::config(format!("config read failed: {e}"))
                        .into_cli_error()
                }),
            )
            .await?;
            sender
                .send(json!({ "type": "step", "name": "config.read", "status": "completed", "path": config_path.display().to_string() }))
                .await;

            // Look up the application and its latest release
            sender
                .send(json!({ "type": "step", "name": "application.lookup", "status": "started" }))
                .await;
            let app_data = tap_deploy_err(
                &sender,
                client
                    .get_application_with_releases(&name)
                    .await
                    .map_err(super::client_err),
            )
            .await?;
            let app = &app_data["application"];
            if app.is_null() {
                let err =
                    crate::error::GddyError::not_found(format!("application '{name}' not found"))
                        .into_cli_error();
                return Err(fail_deploy(&sender, err).await);
            }
            let application_id = app["id"].as_str().unwrap_or("").to_owned();
            let application_status = app["status"].as_str().map(str::to_owned);
            sender
                .send(json!({ "type": "step", "name": "application.lookup", "status": "completed", "id": application_id }))
                .await;

            // Resolve latest release
            sender
                .send(json!({ "type": "step", "name": "release.lookup", "status": "started" }))
                .await;
            let release_id = app["releases"]["edges"]
                .as_array()
                .and_then(|edges| edges.first())
                .and_then(|e| e["node"]["id"].as_str())
                .map(str::to_owned);
            let Some(release_id) = release_id else {
                let err = cli_engine::CliCoreError::message(format!(
                    "application '{name}' has no releases — create one first with: gddy platform app release --application-id {application_id} --version 0.0.1"
                ));
                return Err(fail_deploy(&sender, err).await);
            };
            sender
                .send(json!({ "type": "step", "name": "release.lookup", "status": "completed", "releaseId": release_id }))
                .await;

            // Process each extension
            let deploy_extensions = extensions::collect_extensions(&config);
            let total = deploy_extensions.len();
            // One timestamp for the whole deploy so all artifacts share a temp root.
            let deploy_timestamp = crate::extension::format_timestamp(chrono::Utc::now());
            sender
                .send(json!({ "type": "step", "name": "extensions", "status": "started", "total": total }))
                .await;

            for (i, ext) in deploy_extensions.iter().enumerate() {
                tap_deploy_err(
                    &sender,
                    extensions::deploy_extension(
                        &client,
                        &sender,
                        DeployExtensionArgs {
                            application_id: &application_id,
                            release_id: &release_id,
                            ext,
                            index: i + 1,
                            total,
                            deploy_timestamp: &deploy_timestamp,
                        },
                    )
                    .await,
                )
                .await?;
            }

            tap_deploy_err(
                &sender,
                activate_release(&client, &sender, &application_id, &release_id).await,
            )
            .await?;

            tap_deploy_err(
                &sender,
                activate_application(
                    &client,
                    &sender,
                    &application_id,
                    application_status.as_deref(),
                )
                .await,
            )
            .await?;

            tap_deploy_err(
                &sender,
                sync_manifest_metadata(&client, &sender, &application_id, &config).await,
            )
            .await?;

            sender
                .send(json!({
                    "type": "step",
                    "name": "deploy",
                    "status": "completed",
                    "application": name,
                    "extensions": total,
                }))
                .await;

            sender
                .send(deploy_result_event(
                    &name,
                    &application_id,
                    &release_id,
                    total,
                ))
                .await;
            Ok(())
        },
    )
}

/// Activate the release before activating the parent application.
async fn activate_release(
    client: &ApplicationClient,
    sender: &StreamSender,
    application_id: &str,
    release_id: &str,
) -> cli_engine::Result<()> {
    sender
        .send(json!({ "type": "step", "name": "release.activate", "status": "started" }))
        .await;
    client
        .activate_release(application_id, release_id)
        .await
        .map_err(super::client_err)?;
    sender
        .send(json!({ "type": "step", "name": "release.activate", "status": "completed" }))
        .await;

    Ok(())
}

/// Promote the parent application to `ACTIVE` after its release is active.
/// Avoid the lifecycle mutation when lookup already returned `ACTIVE`.
async fn activate_application(
    client: &ApplicationClient,
    sender: &StreamSender,
    application_id: &str,
    current_status: Option<&str>,
) -> cli_engine::Result<()> {
    if !application_needs_activation(current_status) {
        sender
            .send(json!({
                "type": "step",
                "name": "application.activate",
                "status": "skipped",
                "reason": "already active",
            }))
            .await;
        return Ok(());
    }

    sender
        .send(json!({ "type": "step", "name": "application.activate", "status": "started" }))
        .await;
    client
        .update_application(application_id, json!({ "status": "ACTIVE" }))
        .await
        .map_err(super::client_err)?;
    sender
        .send(json!({ "type": "step", "name": "application.activate", "status": "completed" }))
        .await;

    Ok(())
}

fn application_needs_activation(current_status: Option<&str>) -> bool {
    current_status != Some("ACTIVE")
}

/// Synchronize application-level manifest fields without changing lifecycle
/// status. Actions and subscriptions belong to the release and are handled by
/// `platform app release`.
async fn sync_manifest_metadata(
    client: &ApplicationClient,
    sender: &StreamSender,
    application_id: &str,
    config: &crate::config::Config,
) -> cli_engine::Result<()> {
    sender
        .send(json!({ "type": "step", "name": "application.sync", "status": "started" }))
        .await;
    client
        .update_application(application_id, manifest_metadata_input(config))
        .await
        .map_err(super::client_err)?;
    sender
        .send(json!({ "type": "step", "name": "application.sync", "status": "completed" }))
        .await;
    Ok(())
}

fn manifest_metadata_input(config: &crate::config::Config) -> Value {
    json!({
        "url": config.url,
        "proxyUrl": config.proxy_url,
        "authorizationScopes": config.authorization_scopes,
    })
}

#[cfg(test)]
mod tests {
    /// `deploy --follow` must end with exactly one terminal line. `StreamSender`
    /// has no public constructor outside cli-engine, so the send itself can't
    /// be unit-tested here — instead this locks down the shape of the payload
    /// that gets sent, which is where the actual logic (error-code derivation)
    /// lives.
    #[test]
    fn deploy_error_event_falls_back_to_error_code_for_a_plain_message() {
        let err = cli_engine::CliCoreError::message("application 'foo' not found");
        let event = super::deploy_error_event(&err);

        assert_eq!(event["type"], "error");
        assert_eq!(event["ok"], false);
        assert_eq!(event["error"]["code"], "ERROR");
        assert_eq!(event["error"]["message"], "application 'foo' not found");
        assert!(event.get("fix").is_none());
        assert_eq!(event["next_actions"], serde_json::json!([]));
    }

    #[test]
    fn deploy_error_event_includes_fix_from_gddy_error() {
        let err =
            crate::error::GddyError::not_found("application 'foo' not found").into_cli_error();
        let event = super::deploy_error_event(&err);

        assert_eq!(event["error"]["code"], crate::error::codes::NOT_FOUND);
        assert_eq!(event["error"]["message"], "application 'foo' not found");
        assert!(
            event["fix"]
                .as_str()
                .is_some_and(|f| f.contains("platform app list")),
            "expected fix on streaming error event: {event}"
        );
    }

    #[derive(Debug)]
    struct CodedTestError;

    impl std::fmt::Display for CodedTestError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "upstream rejected the release")
        }
    }

    impl std::error::Error for CodedTestError {}

    impl cli_engine::DetailedError for CodedTestError {
        fn error_code(&self) -> std::borrow::Cow<'static, str> {
            "RELEASE_REJECTED".into()
        }

        fn error_system(&self) -> Option<std::borrow::Cow<'static, str>> {
            Some("applications".into())
        }

        fn error_request_id(&self) -> Option<std::borrow::Cow<'static, str>> {
            None
        }

        fn error_fix(&self) -> Option<std::borrow::Cow<'static, str>> {
            Some("Check release status and retry.".into())
        }
    }

    /// Proves `deploy_error_event` actually passes through a real error code
    /// (via `build_error_envelope`) rather than always falling back — the
    /// fallback-only case above wouldn't catch a regression that hardcoded
    /// `"ERROR"`.
    #[test]
    fn deploy_error_event_passes_through_a_real_error_code() {
        let err = cli_engine::CliCoreError::with_detailed_error(CodedTestError);
        let event = super::deploy_error_event(&err);

        assert_eq!(event["error"]["code"], "RELEASE_REJECTED");
        assert_eq!(event["error"]["message"], "upstream rejected the release");
        assert_eq!(event["fix"], "Check release status and retry.");
    }

    #[test]
    fn deploy_result_event_is_ok_with_summary_fields() {
        let event = super::deploy_result_event("my-app", "app-123", "rel-456", 2);

        assert_eq!(event["type"], "result");
        assert_eq!(event["ok"], true);
        assert_eq!(event["result"]["application"], "my-app");
        assert_eq!(event["result"]["applicationId"], "app-123");
        assert_eq!(event["result"]["releaseId"], "rel-456");
        assert_eq!(event["result"]["extensions"], 2);
        assert_eq!(event["result"]["releaseStatus"], "ACTIVE");
        assert_eq!(event["result"]["applicationStatus"], "ACTIVE");
        assert_eq!(
            event["next_actions"].as_array().map(|a| a.len()),
            Some(3),
            "deploy should suggest enable, info, and redeploy: {event}"
        );
        assert_eq!(
            event["next_actions"][1]["params"]["name"],
            serde_json::json!({ "value": "my-app", "required": true }),
            "deployed application name should prefill the next action: {event}"
        );
        assert!(
            event["next_actions"][0]["command"]
                .as_str()
                .unwrap_or("")
                .contains("platform app enable"),
            "first next action should enable on a store: {event}"
        );
    }

    #[test]
    fn application_activation_is_skipped_only_when_already_active() {
        assert!(!super::application_needs_activation(Some("ACTIVE")));
        assert!(super::application_needs_activation(Some("INACTIVE")));
        assert!(super::application_needs_activation(Some("VERIFYING")));
        assert!(super::application_needs_activation(None));
    }

    #[test]
    fn manifest_metadata_input_syncs_all_application_fields_without_status() {
        let config = crate::config::Config {
            name: "my-app".to_owned(),
            client_id: "550e8400-e29b-41d4-a716-446655440000".to_owned(),
            description: Some("test".to_owned()),
            version: "1.2.3".to_owned(),
            url: "https://app.example.com".to_owned(),
            proxy_url: "https://api.example.com".to_owned(),
            authorization_scopes: vec!["openid".to_owned(), "profile".to_owned()],
            actions: vec![],
            subscriptions: None,
            dependencies: vec![],
            extensions: None,
            settings: vec![],
        };

        let input = super::manifest_metadata_input(&config);

        assert_eq!(input["url"], "https://app.example.com");
        assert_eq!(input["proxyUrl"], "https://api.example.com");
        assert_eq!(
            input["authorizationScopes"],
            serde_json::json!(["openid", "profile"])
        );
        assert!(input.get("status").is_none());
    }
}
