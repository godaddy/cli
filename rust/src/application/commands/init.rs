//! `gddy platform app init` — register a new application and write godaddy.toml.

use cli_engine::{
    CommandResult, CommandSpec, NextActionParam, RuntimeCommandSpec, TableColumn, Tier,
};
use serde_json::json;

use super::schemas::ApplicationInit;
use crate::next_action::{next_action, required_value};
use crate::scopes::{APP_REGISTRY_READ, APP_REGISTRY_WRITE};

#[derive(Debug, Clone, clap::Args)]
struct InitArgs {
    /// Application name (used as label if --label is not set).
    #[arg(long, short = 'n', value_name = "NAME")]
    name: Option<String>,

    /// Display label (defaults to name).
    #[arg(long, value_name = "LABEL")]
    label: Option<String>,

    /// Application description.
    #[arg(long, value_name = "TEXT")]
    description: Option<String>,

    /// Application URL (must be public HTTP(S)).
    #[arg(long, value_name = "URL")]
    url: Option<String>,

    /// Proxy URL (must be public HTTP(S)).
    #[arg(long, value_name = "URL")]
    proxy_url: Option<String>,

    /// Comma-separated authorization scopes.
    #[arg(long, value_name = "SCOPES")]
    scopes: Option<String>,

    /// Read defaults from this config file.
    #[arg(long, short = 'c', value_name = "PATH")]
    config: Option<String>,

    /// Accept GoDaddy Developer agreements non-interactively when onboarding
    /// is still pending (required for non-TTY).
    #[arg(long)]
    accept_agreements: bool,
}

/// `filesWritten` is a small path-by-kind object (`config`/`env`), so it
/// renders as an indented property bag instead of a raw JSON dump.
fn init_view_columns() -> Vec<TableColumn> {
    vec![
        TableColumn::new("id", "ID"),
        TableColumn::new("name", "Name"),
        TableColumn::new("status", "Status"),
        TableColumn::new("clientId", "Client ID"),
        TableColumn::new("orgId", "Org ID"),
        TableColumn::new("url", "URL").no_truncate(true),
        TableColumn::new("proxyUrl", "Proxy URL").no_truncate(true),
        TableColumn::new("authorizationScopes", "Authorization Scopes"),
        TableColumn::new("oauthGrantTypes", "OAuth Grant Types"),
        TableColumn::new("filesWritten", "Files Written").nested(vec![
            TableColumn::new("config", "Config").no_truncate(true),
            TableColumn::new("env", "Env").no_truncate(true),
        ]),
    ]
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<InitArgs, _, _, _>(
        CommandSpec::from_args::<InitArgs>("init", "Create and initialize a new application")
            .with_long(
                "Register a new GoDaddy developer-platform application and write a \
                godaddy.toml manifest to the current directory. The manifest captures the \
                application name, URL, authorization scopes, and any actions or extensions \
                added later. Run `gddy platform app validate <name>` to confirm the remote \
                application is healthy, then `gddy platform app release` to create a versioned \
                release.",
            )
            .with_system("applications")
            .with_tier(Tier::Mutate)
            .with_scopes(&[APP_REGISTRY_READ, APP_REGISTRY_WRITE])
            .with_output_schema::<ApplicationInit>()
            .with_view(init_view_columns()),
        |ctx, args: InitArgs| async move {
            let env = ctx.middleware.env.clone();
            let config_path = crate::config::config_path(Some(&env));
            let accept_agreements = args.accept_agreements;

            // Seed defaults only from an explicit --config; a bad/missing --config is fatal.
            let existing = match args.config.as_deref() {
                Some(path) => Some(
                    crate::config::read_config(std::path::Path::new(path)).map_err(|e| {
                        crate::error::GddyError::config(format!("invalid config: {e}"))
                            .into_cli_error()
                    })?,
                ),
                None => None,
            };

            // Resolve each field: CLI flag, else the existing config's value, else a default.
            let name = args
                .name
                .or_else(|| existing.as_ref().map(|c| c.name.clone()))
                .unwrap_or_default();
            let description = args
                .description
                .or_else(|| existing.as_ref().and_then(|c| c.description.clone()))
                .unwrap_or_default();
            let url = args
                .url
                .or_else(|| existing.as_ref().map(|c| c.url.clone()))
                .unwrap_or_default();
            let proxy_url = args
                .proxy_url
                .or_else(|| existing.as_ref().map(|c| c.proxy_url.clone()))
                .unwrap_or_default();
            let scopes: Vec<String> = args
                .scopes
                .map(|s| {
                    s.split(',')
                        .map(|p| p.trim())
                        .filter(|p| !p.is_empty())
                        .map(str::to_owned)
                        .collect()
                })
                .or_else(|| existing.as_ref().map(|c| c.authorization_scopes.clone()))
                .unwrap_or_default();
            let label = args.label.unwrap_or_else(|| name.clone());

            for (message, empty) in [
                ("Application name is required", name.is_empty()),
                (
                    "Application description is required",
                    description.is_empty(),
                ),
                ("Application URL is required", url.is_empty()),
                ("Proxy URL is required", proxy_url.is_empty()),
                ("Authorization scopes are required", scopes.is_empty()),
            ] {
                if empty {
                    return Err(cli_engine::CliCoreError::message(message));
                }
            }

            // Reject names that cannot be written to a valid godaddy.toml.
            if !crate::config::is_valid_app_name(&name) {
                return Err(crate::error::GddyError::validation(format!(
                    "Application name must be 3-255 lowercase letters, digits, or hyphens \
                     (got {name:?})"
                ))
                .into_cli_error());
            }

            for (field, u) in [("url", &url), ("proxyUrl", &proxy_url)] {
                if !crate::application::public_url::is_public_routable_url(u) {
                    return Err(cli_engine::CliCoreError::message(format!(
                        "Invalid application configuration: {field} must be a publicly-resolvable \
                         http(s) URL (localhost, loopback, and private IPs are not allowed)"
                    )));
                }
            }

            let credential = ctx.credential().await?;
            let onboarding = crate::onboarding::ensure_ready_for_app_init(
                &credential.token,
                &env,
                accept_agreements,
            )
            .await?;

            let client = super::make_client(&ctx).await?;
            let data = client
                .create_application(json!({
                    "name": name,
                    "label": label,
                    "description": description,
                    "url": url,
                    "proxyUrl": proxy_url,
                    "organizationId": &onboarding.org_id,
                    "authorizationScopes": scopes,
                }))
                .await
                .map_err(super::client_err)?;

            let app = &data["createApplication"];
            // Credentials/secrets the API returns (all selected by create_application).
            let client_id = app["clientId"].as_str().unwrap_or("").to_owned();
            let client_secret = app["clientSecret"].as_str().unwrap_or("").to_owned();
            let secret = app["secret"].as_str().unwrap_or("").to_owned();
            let public_key = app["publicKey"].as_str().unwrap_or("").to_owned();

            // Best-effort local writes: app create already succeeded. Only paths that
            // actually write are included in filesWritten.
            let config = crate::config::Config {
                name: name.clone(),
                client_id: client_id.clone(),
                description: Some(description.clone()),
                version: "0.0.0".to_owned(),
                url: url.clone(),
                proxy_url: proxy_url.clone(),
                authorization_scopes: scopes.clone(),
                actions: vec![],
                subscriptions: Some(crate::config::SubscriptionsConfig { webhook: vec![] }),
                dependencies: vec![],
                extensions: None,
                settings: vec![],
                native_extension: None,
            };
            let cwd = match std::env::current_dir() {
                Ok(dir) => dir,
                Err(e) => {
                    tracing::debug!(
                        error = %e,
                        "current_dir failed; filesWritten paths will be relative"
                    );
                    std::path::PathBuf::new()
                }
            };

            let mut files_written = serde_json::Map::new();
            if let Err(e) = crate::config::write_config(&config_path, &config) {
                tracing::warn!(error = %e, path = %config_path.display(), "failed to write config; continuing");
            } else {
                files_written.insert(
                    "config".to_owned(),
                    json!(cwd.join(&config_path).display().to_string()),
                );
            }

            let env_file_path = crate::config::env_path(Some(&env));
            if let Err(e) = crate::config::write_env_file(
                Some(&env),
                &secret,
                &public_key,
                &client_id,
                &client_secret,
            ) {
                tracing::warn!(error = %e, path = %env_file_path.display(), "failed to write env file; continuing");
            } else {
                files_written.insert(
                    "env".to_owned(),
                    json!(cwd.join(&env_file_path).display().to_string()),
                );
            }

            let app_id = app["id"].as_str().unwrap_or("").to_owned();
            let result = json!({
                "id": app_id,
                "name": name,
                "status": app["status"].as_str().unwrap_or("").to_owned(),
                "clientId": client_id,
                "orgId": onboarding.org_id,
                "url": url,
                "proxyUrl": proxy_url,
                "authorizationScopes": scopes,
                "oauthGrantTypes": ["authorization_code", "client_credentials"],
                "filesWritten": files_written,
            });
            Ok(CommandResult::new(result).with_next_actions(vec![
                next_action(
                    "platform app add action --name <name> --url <url>",
                    "Add first action",
                ),
                next_action(
                    "platform app add subscription --name <name> --events <events> --url <url>",
                    "Add webhook subscription",
                ),
                next_action(
                    "platform app validate <name>",
                    "Validate the remote application state",
                )
                .with_param("name", required_value(&name)),
                next_action(
                    "platform app release --application-id <application-id> --version <version>",
                    "Create the first release",
                )
                .with_param("application-id", required_value(&app_id))
                .with_param("version", NextActionParam::required()),
            ]))
        },
    )
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::init_view_columns;

    /// Proves `init_view_columns()` renders a `filesWritten` shaped like what
    /// the `init` handler actually builds (`config`/`env` paths, confirmed by
    /// inspection) as a nested property bag — a column/field name mismatch
    /// would silently drop the write summary from human output and print a
    /// raw JSON blob instead. Renders a hand-built envelope rather than
    /// calling the handler, which would need a live app-registry API call.
    #[test]
    fn init_result_renders_files_written_as_a_nested_property_bag() {
        let result = json!({
            "id": "app-1",
            "name": "demo",
            "status": "ACTIVE",
            "filesWritten": {
                "config": "/home/user/project/godaddy.toml",
                "env": "/home/user/project/.env",
            },
        });
        let envelope = cli_engine::Envelope::success(result, "applications");
        let rendered =
            cli_engine::render_human_with_view(&envelope, Some(&init_view_columns()), "");
        assert!(rendered.contains("Files Written:"), "{rendered}");
        assert!(rendered.contains("godaddy.toml"), "{rendered}");
        assert!(rendered.contains(".env"), "{rendered}");
    }
}
