use cli_engine::{
    CommandResult, CommandSpec, GroupSpec, NextAction, NextActionParam, RuntimeCommandSpec,
    RuntimeGroupSpec, StreamSender, Tier,
};
use serde_json::{Value, json};

use crate::application::client::{ApplicationClient, UploadOptions, api_url_for_env};
use crate::next_action::{next_action, required_value};
use crate::output_schema::output_schema;
// App-registry mutations declare their scopes so `apps.app-registry:write` is
// requested on demand (OAuth step-up), not granted at every login — it's a
// rarely-used operation for most customers.
use crate::scopes::{APP_REGISTRY_READ, APP_REGISTRY_WRITE};

output_schema!(ApplicationSummary {
    "id": "string";
    "name": "string";
    "label": "string", optional;
    "description": "string", optional;
    "status": "string";
    "url": "string", optional;
    "proxyUrl": "string", optional;
});

output_schema!(ApplicationInit {
    "id": "string";
    "name": "string";
    "status": "string";
    "clientId": "string";
    "url": "string";
    "proxyUrl": "string";
    "authorizationScopes": "[]string";
    "oauthGrantTypes": "[]string";
    "filesWritten": "object";
});

output_schema!(ApplicationUpdate {
    "id": "string";
    "clientId": "string";
    "name": "string";
    "label": "string", optional;
    "description": "string", optional;
    "status": "string";
    "url": "string", optional;
    "proxyUrl": "string", optional;
    "authorizationScopes": "[]string";
});

output_schema!(ApplicationRef {
    "id": "string";
});

output_schema!(ApplicationArchive {
    "id": "string";
    "name": "string";
    "label": "string", optional;
    "status": "string";
    "createdAt": "string";
    "archivedAt": "string";
});

output_schema!(ApplicationRelease {
    "id": "string";
    "version": "string";
    "description": "string", optional;
    "createdAt": "string";
});

output_schema!(ValidationResult {
    "valid": "bool";
    "errors": "[]string";
    "warnings": "[]string";
});

output_schema!(ConfigAction {
    "name": "string";
    "url": "string";
});

output_schema!(ConfigSubscription {
    "name": "string";
    "url": "string";
    "events": "[]string";
});

output_schema!(ExtensionHandle {
    "name": "string";
    "handle": "string";
    "type": "string";
});

output_schema!(ExtensionBlocks {
    "source": "string";
    "type": "string";
});

async fn make_client(ctx: &cli_engine::CommandContext) -> cli_engine::Result<ApplicationClient> {
    // Lazily resolve the credential; this triggers the auth flow only for
    // commands that actually call the API.
    let token = ctx.credential().await?.token;
    let base_url = api_url_for_env(&ctx.middleware.env);
    Ok(ApplicationClient::new(base_url, token))
}

fn client_err(e: crate::application::client::ClientError) -> cli_engine::CliCoreError {
    cli_engine::CliCoreError::message(e.to_string())
}

/// Builds the terminal `{"type":"error",...}` event for a streaming command,
/// reusing `cli_engine::build_error_envelope` so the `code`/`message` match
/// what a non-streaming command would have rendered for the same error.
fn deploy_error_event(err: &cli_engine::CliCoreError) -> Value {
    let envelope = cli_engine::build_error_envelope(err, "applications");
    // `build_error_envelope` always populates `.error` with a non-empty code;
    // this fallback only guards against a future change to that guarantee.
    let (code, message) = envelope
        .error
        .map(|e| (e.code, e.message))
        .unwrap_or_else(|| ("ERROR".to_owned(), err.to_string()));
    json!({
        "type": "error",
        "ok": false,
        "error": { "code": code, "message": message },
        "next_actions": [],
    })
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

/// Builds the terminal `{"type":"result",...}` event for a successful
/// `application deploy` run.
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
            "status": "ACTIVE",
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

fn arg_str<'a>(ctx: &'a cli_engine::CommandContext, key: &str) -> &'a str {
    ctx.args.get(key).and_then(|v| v.as_str()).unwrap_or("")
}

/// Next-actions after mutating local godaddy.toml (add action/subscription/extension).
fn add_config_next_actions(app_name: &str) -> Vec<NextAction> {
    let name_param = if app_name.is_empty() {
        NextActionParam::required()
    } else {
        required_value(app_name)
    };
    vec![
        next_action(
            "application validate <name>",
            "Validate remote application configuration",
        )
        .with_param("name", name_param),
        next_action(
            "application release --application-id <application-id> --version <version>",
            "Create a new release",
        )
        .with_param("application-id", NextActionParam::required())
        .with_param("version", NextActionParam::required()),
    ]
}

/// Next-actions after a successful deploy.
fn deploy_next_actions(name: &str) -> Vec<NextAction> {
    vec![
        next_action(
            "application enable <name> --store-id <store-id>",
            "Enable the application on a store",
        )
        .with_param("name", required_value(name))
        .with_param("store-id", NextActionParam::required()),
        next_action(
            "application info --name <name>",
            "Inspect deployment status",
        )
        .with_param("name", required_value(name)),
        next_action("application deploy --name <name>", "Rerun deployment")
            .with_param("name", required_value(name)),
    ]
}

pub fn application_group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(
        GroupSpec::new("application", "Manage GoDaddy applications")
            .with_long(
                "Manage GoDaddy developer-platform applications. A GoDaddy application is a \
                developer-platform app described by a godaddy.toml manifest in your working \
                directory. Use `application init` to create one, `application validate <name>` \
                to check remote application state, and `application deploy` to publish it.",
            )
            .with_alias("app"),
    )
    .with_command(list_command())
    .with_command(info_command())
    .with_command(init_command())
    .with_command(validate_command())
    .with_command(update_command())
    .with_command(enable_command())
    .with_command(disable_command())
    .with_command(archive_command())
    .with_command(release_command())
    .with_command(deploy_command())
    .with_group(add_group())
}

fn list_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_with_context(
        CommandSpec::new("list", "List all applications")
            .with_long(
                "List all GoDaddy developer-platform applications visible to the current \
                account. Use `application info --name <name>` to fetch full details for a \
                single application.",
            )
            .with_system("applications")
            .with_tier(Tier::Read)
            .with_default_fields("name,label,status")
            .with_output_schema::<ApplicationSummary>(),
        |ctx| async move {
            let client = make_client(&ctx).await?;
            let data = client.list_applications().await.map_err(client_err)?;
            Ok(CommandResult::new(data).with_next_actions(vec![
                next_action(
                    "application info --name <name>",
                    "Get details for a specific application",
                )
                .with_param("name", NextActionParam::required()),
                next_action(
                    "application init --name <name> --description <description> --url <url> --proxy-url <proxy-url> --scopes <scopes>",
                    "Initialize a new application",
                ),
            ]))
        },
    )
}

fn info_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_with_context(
        CommandSpec::new("info", "Get application details")
            .with_long(
                "Fetch full details for a single GoDaddy developer-platform application by \
                name, including status, URLs, and authorization scopes. Use `application list` \
                to find available names.",
            )
            .with_system("applications")
            .with_tier(Tier::Read)
            .with_output_schema::<ApplicationSummary>()
            .with_arg(
                clap::Arg::new("name")
                    .long("name")
                    .short('n')
                    .value_name("NAME")
                    .required(true)
                    .help("Application name"),
            ),
        |ctx| async move {
            let name = arg_str(&ctx, "name").to_owned();
            let client = make_client(&ctx).await?;
            let data = client.get_application(&name).await.map_err(client_err)?;
            let app = &data["application"];
            if app.is_null() {
                return Err(cli_engine::CliCoreError::message(format!(
                    "application '{name}' not found"
                )));
            }
            let app_id = app["id"].as_str().unwrap_or("").to_owned();
            Ok(CommandResult::new(app.clone()).with_next_actions(vec![
                next_action(
                    "application validate <name>",
                    "Validate application configuration",
                )
                .with_param("name", required_value(&name)),
                next_action(
                    "application update --id <id> [--label <label>] [--description <description>] [--status <status>]",
                    "Update application configuration",
                )
                .with_param("id", required_value(&app_id))
                .with_param(
                    "status",
                    NextActionParam {
                        r#enum: vec!["ACTIVE".to_owned(), "INACTIVE".to_owned()],
                        ..Default::default()
                    },
                ),
                next_action(
                    "application release --application-id <application-id> --version <version>",
                    "Create a release",
                )
                .with_param("application-id", required_value(&app_id))
                .with_param("version", NextActionParam::required()),
                next_action(
                    "application deploy --name <name>",
                    "Deploy this application",
                )
                .with_param("name", required_value(&name)),
            ]))
        },
    )
}

fn init_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_with_context(
        CommandSpec::new("init", "Create and initialize a new application")
            .with_long(
                "Register a new GoDaddy developer-platform application and write a \
                godaddy.toml manifest to the current directory. The manifest captures the \
                application name, URL, authorization scopes, and any actions or extensions \
                added later. Run `application validate <name>` to confirm the remote \
                application is healthy, then `application release` to create a versioned \
                release.",
            )
            .with_system("applications")
            .with_tier(Tier::Mutate)
            .with_scopes(&[APP_REGISTRY_READ, APP_REGISTRY_WRITE])
            .with_output_schema::<ApplicationInit>()
            .with_arg(
                clap::Arg::new("name")
                    .long("name")
                    .short('n')
                    .value_name("NAME")
                    .help("Application name (used as label if --label is not set)"),
            )
            .with_arg(
                clap::Arg::new("label")
                    .long("label")
                    .value_name("LABEL")
                    .help("Display label (defaults to name)"),
            )
            .with_arg(
                clap::Arg::new("description")
                    .long("description")
                    .value_name("TEXT")
                    .help("Application description"),
            )
            .with_arg(
                clap::Arg::new("url")
                    .long("url")
                    .value_name("URL")
                    .help("Application URL (must be public HTTP(S))"),
            )
            .with_arg(
                clap::Arg::new("proxy-url")
                    .long("proxy-url")
                    .value_name("URL")
                    .help("Proxy URL (must be public HTTP(S))"),
            )
            .with_arg(
                clap::Arg::new("scopes")
                    .long("scopes")
                    .value_name("SCOPES")
                    .help("Comma-separated authorization scopes"),
            )
            .with_arg(
                clap::Arg::new("config")
                    .long("config")
                    .short('c')
                    .value_name("PATH")
                    .help("Read defaults from this config file"),
            ),
        |ctx| async move {
            let env = ctx.middleware.env.clone();
            let config_path = crate::config::config_path(Some(&env));

            // Seed defaults only from an explicit --config; a bad/missing --config is fatal.
            let existing = match ctx.args.get("config").and_then(|v| v.as_str()) {
                Some(path) => Some(
                    crate::config::read_config(std::path::Path::new(path)).map_err(|e| {
                        cli_engine::CliCoreError::message(format!("invalid config: {e}"))
                    })?,
                ),
                None => None,
            };

            // Resolve each field: CLI flag, else the existing config's value, else a default.
            let flag = |key: &str| ctx.args.get(key).and_then(|v| v.as_str());
            let name = flag("name")
                .map(str::to_owned)
                .or_else(|| existing.as_ref().map(|c| c.name.clone()))
                .unwrap_or_default();
            let description = flag("description")
                .map(str::to_owned)
                .or_else(|| existing.as_ref().and_then(|c| c.description.clone()))
                .unwrap_or_default();
            let url = flag("url")
                .map(str::to_owned)
                .or_else(|| existing.as_ref().map(|c| c.url.clone()))
                .unwrap_or_default();
            let proxy_url = flag("proxy-url")
                .map(str::to_owned)
                .or_else(|| existing.as_ref().map(|c| c.proxy_url.clone()))
                .unwrap_or_default();
            let scopes: Vec<String> = flag("scopes")
                .map(|s| {
                    s.split(',')
                        .map(str::trim)
                        .filter(|p| !p.is_empty())
                        .map(str::to_owned)
                        .collect()
                })
                .or_else(|| existing.as_ref().map(|c| c.authorization_scopes.clone()))
                .unwrap_or_default();
            let label = flag("label").unwrap_or(&name).to_owned();

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

            for (field, u) in [("url", &url), ("proxyUrl", &proxy_url)] {
                if !crate::application::public_url::is_public_routable_url(u) {
                    return Err(cli_engine::CliCoreError::message(format!(
                        "Invalid application configuration: {field} must be a publicly-resolvable \
                         http(s) URL (localhost, loopback, and private IPs are not allowed)"
                    )));
                }
            }

            let client = make_client(&ctx).await?;
            let data = client
                .create_application(json!({
                    "name": name,
                    "label": label,
                    "description": description,
                    "url": url,
                    "proxyUrl": proxy_url,
                    "authorizationScopes": scopes,
                }))
                .await
                .map_err(client_err)?;

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
                "url": url,
                "proxyUrl": proxy_url,
                "authorizationScopes": scopes,
                "oauthGrantTypes": ["authorization_code", "client_credentials"],
                "filesWritten": files_written,
            });
            Ok(CommandResult::new(result).with_next_actions(vec![
                next_action(
                    "application add action --name <name> --url <url>",
                    "Add first action",
                ),
                next_action(
                    "application add subscription --name <name> --events <events> --url <url>",
                    "Add webhook subscription",
                ),
                next_action(
                    "application validate <name>",
                    "Validate the remote application state",
                )
                .with_param("name", required_value(&name)),
                next_action(
                    "application release --application-id <application-id> --version <version>",
                    "Create the first release",
                )
                .with_param("application-id", required_value(&app_id))
                .with_param("version", NextActionParam::required()),
            ]))
        },
    )
}

/// Missing URL is an error; missing proxy URL or INACTIVE status are warnings.
fn validate_remote_application(app: &Value) -> (bool, Vec<String>, Vec<String>) {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    let url = app["url"].as_str().unwrap_or("");
    if url.is_empty() {
        errors.push("Application URL is required".to_owned());
    }
    let proxy_url = app["proxyUrl"].as_str().unwrap_or("");
    if proxy_url.is_empty() {
        warnings.push("Proxy URL is not set".to_owned());
    }
    if app["status"].as_str() == Some("INACTIVE") {
        warnings.push("Application is currently inactive".to_owned());
    }

    (errors.is_empty(), errors, warnings)
}

fn validate_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_with_context(
        CommandSpec::new("validate", "Validate remote application state")
            .with_long(
                "Fetch a GoDaddy developer-platform application by name and validate its \
                remote configuration. Reports an error when the application URL is missing, \
                and warnings when the proxy URL is unset or the application is inactive. \
                Requires authentication.",
            )
            .with_system("applications")
            .with_tier(Tier::Read)
            .with_output_schema::<ValidationResult>()
            .with_arg(
                clap::Arg::new("name")
                    .value_name("NAME")
                    .required(true)
                    .help("Application name"),
            ),
        |ctx| async move {
            let name = arg_str(&ctx, "name").to_owned();
            let client = make_client(&ctx).await?;
            let data = client.get_application(&name).await.map_err(client_err)?;
            let app = &data["application"];
            if app.is_null() {
                return Err(cli_engine::CliCoreError::message(format!(
                    "application '{name}' not found"
                )));
            }

            let app_id = app["id"].as_str().unwrap_or("").to_owned();
            let (valid, errors, warnings) = validate_remote_application(app);

            // Only suggest a release once the app is valid; otherwise point back to
            // `info` to review the reported problems.
            let mut next_actions = Vec::new();
            if valid {
                next_actions.push(
                    next_action(
                        "application release --application-id <application-id> --version <version>",
                        "Create a release after validation",
                    )
                    .with_param("application-id", required_value(&app_id))
                    .with_param("version", NextActionParam::required()),
                );
            }
            next_actions.push(
                next_action(
                    "application info --name <name>",
                    "Inspect application details",
                )
                .with_param("name", required_value(&name)),
            );

            Ok(CommandResult::new(json!({
                "valid": valid,
                "errors": errors,
                "warnings": warnings,
            }))
            .with_next_actions(next_actions))
        },
    )
}

fn update_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_with_context(
        CommandSpec::new("update", "Update an application")
            .with_long(
                "Update the label, description, or status of a GoDaddy developer-platform \
                application by its ID. At least one of --label, --description, or --status \
                must be provided. Use `application info --name <name>` to retrieve the \
                application ID.",
            )
            .with_system("applications")
            .with_tier(Tier::Mutate)
            .with_scopes(&[APP_REGISTRY_READ, APP_REGISTRY_WRITE])
            .with_output_schema::<ApplicationUpdate>()
            .with_arg(
                clap::Arg::new("id")
                    .long("id")
                    .value_name("ID")
                    .required(true)
                    .help("Application ID"),
            )
            .with_arg(
                clap::Arg::new("label")
                    .long("label")
                    .value_name("LABEL")
                    // CommandSpec has no ArgGroup; this enforces "at least one" at parse time.
                    .required_unless_present_any(["description", "status"])
                    .help("New label"),
            )
            .with_arg(
                clap::Arg::new("description")
                    .long("description")
                    .value_name("TEXT")
                    .required_unless_present_any(["label", "status"])
                    .help("New description"),
            )
            .with_arg(
                clap::Arg::new("status")
                    .long("status")
                    .value_name("STATUS")
                    .value_parser(["ACTIVE", "INACTIVE"])
                    .required_unless_present_any(["label", "description"])
                    .help("Application status (ACTIVE or INACTIVE)"),
            ),
        |ctx| async move {
            let id = arg_str(&ctx, "id").to_owned();
            let mut input = serde_json::Map::new();
            if let Some(label) = ctx.args.get("label").and_then(|v| v.as_str()) {
                input.insert("label".to_owned(), json!(label));
            }
            if let Some(desc) = ctx.args.get("description").and_then(|v| v.as_str()) {
                input.insert("description".to_owned(), json!(desc));
            }
            if let Some(status) = ctx.args.get("status").and_then(|v| v.as_str()) {
                input.insert("status".to_owned(), json!(status));
            }
            let client = make_client(&ctx).await?;
            let data = client
                .update_application(&id, json!(input))
                .await
                .map_err(client_err)?;
            let name = data["updateApplication"]["name"]
                .as_str()
                .unwrap_or("")
                .to_owned();
            Ok(
                CommandResult::new(data["updateApplication"].clone()).with_next_actions(vec![
                    next_action(
                        "application info --name <name>",
                        "Inspect updated application",
                    )
                    .with_param("name", required_value(&name)),
                    next_action(
                        "application deploy --name <name>",
                        "Deploy updated application",
                    )
                    .with_param("name", required_value(&name)),
                ]),
            )
        },
    )
}

fn enable_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_with_context(
        CommandSpec::new("enable", "Enable an application on a store")
            .with_long(
                "Make a GoDaddy developer-platform application available on a specific \
                store. Use `application disable` to reverse this. Both the application name \
                and a store ID are required.",
            )
            .with_system("applications")
            .with_tier(Tier::Mutate)
            .with_scopes(&[APP_REGISTRY_READ, APP_REGISTRY_WRITE])
            .with_output_schema::<ApplicationRef>()
            .with_arg(
                clap::Arg::new("name")
                    .value_name("NAME")
                    .required(true)
                    .help("Application name"),
            )
            .with_arg(
                clap::Arg::new("store-id")
                    .long("store-id")
                    .value_name("STORE_ID")
                    .required(true)
                    .help("Store ID to enable the application on"),
            ),
        |ctx| async move {
            let name = arg_str(&ctx, "name").to_owned();
            let store_id = arg_str(&ctx, "store-id").to_owned();
            let client = make_client(&ctx).await?;
            let data = client
                .enable_application(json!({ "applicationName": name, "storeId": store_id }))
                .await
                .map_err(client_err)?;
            Ok(
                CommandResult::new(data["enableStoreApplication"].clone()).with_next_actions(vec![
                    next_action(
                        "application disable <name> --store-id <store-id>",
                        "Disable the application on the same store",
                    )
                    .with_param("name", required_value(&name))
                    .with_param("store-id", required_value(&store_id)),
                    next_action(
                        "application info --name <name>",
                        "Inspect application status",
                    )
                    .with_param("name", required_value(&name)),
                ]),
            )
        },
    )
}

fn disable_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_with_context(
        CommandSpec::new("disable", "Disable an application on a store")
            .with_long(
                "Remove a GoDaddy developer-platform application from a specific store, \
                making it unavailable there. Use `application enable` to re-enable it. \
                Both the application name and a store ID are required.",
            )
            .with_system("applications")
            .with_tier(Tier::Mutate)
            .with_scopes(&[APP_REGISTRY_READ, APP_REGISTRY_WRITE])
            .with_output_schema::<ApplicationRef>()
            .with_arg(
                clap::Arg::new("name")
                    .value_name("NAME")
                    .required(true)
                    .help("Application name"),
            )
            .with_arg(
                clap::Arg::new("store-id")
                    .long("store-id")
                    .value_name("STORE_ID")
                    .required(true)
                    .help("Store ID to disable the application on"),
            ),
        |ctx| async move {
            let name = arg_str(&ctx, "name").to_owned();
            let store_id = arg_str(&ctx, "store-id").to_owned();
            let client = make_client(&ctx).await?;
            let data = client
                .disable_application(json!({ "applicationName": name, "storeId": store_id }))
                .await
                .map_err(client_err)?;
            Ok(
                CommandResult::new(data["disableStoreApplication"].clone()).with_next_actions(
                    vec![
                        next_action(
                            "application enable <name> --store-id <store-id>",
                            "Re-enable the application on the same store",
                        )
                        .with_param("name", required_value(&name))
                        .with_param("store-id", required_value(&store_id)),
                        next_action(
                            "application info --name <name>",
                            "Inspect application status",
                        )
                        .with_param("name", required_value(&name)),
                    ],
                ),
            )
        },
    )
}

fn archive_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_with_context(
        CommandSpec::new("archive", "Archive an application")
            .with_long(
                "Archive a GoDaddy developer-platform application by name. Archiving is \
                irreversible via this CLI; the application will no longer be active. \
                Use `application list` to confirm the application name before archiving.",
            )
            .with_system("applications")
            .with_tier(Tier::Destructive)
            .with_scopes(&[APP_REGISTRY_READ, APP_REGISTRY_WRITE])
            .with_output_schema::<ApplicationArchive>()
            .with_arg(
                clap::Arg::new("name")
                    .value_name("NAME")
                    .required(true)
                    .help("Application name"),
            ),
        |ctx| async move {
            let name = arg_str(&ctx, "name").to_owned();
            let client = make_client(&ctx).await?;
            let app_data = client.get_application(&name).await.map_err(client_err)?;
            let app_id = app_data["application"]["id"]
                .as_str()
                .ok_or_else(|| {
                    cli_engine::CliCoreError::message(format!("application '{name}' not found"))
                })?
                .to_owned();
            let data = client
                .archive_application(&app_id)
                .await
                .map_err(client_err)?;
            Ok(
                CommandResult::new(data["archiveApplication"].clone()).with_next_actions(vec![
                    next_action(
                        "application info --name <name>",
                        "Inspect archived application",
                    )
                    .with_param("name", required_value(&name)),
                    next_action("application list", "List all applications"),
                ]),
            )
        },
    )
}

/// Build one `uiExtensions` release entry, enforcing the API's one-target-per-
/// extension limit. `target` is omitted when the extension has no targets.
fn ui_extension_entry(
    name: &str,
    handle: &str,
    source: &str,
    kind: &str,
    targets: &[crate::config::ExtensionTarget],
) -> cli_engine::Result<Value> {
    if targets.len() > 1 {
        return Err(cli_engine::CliCoreError::message(format!(
            "UI extension '{name}' has {} targets, but only one target is supported per extension during release",
            targets.len()
        )));
    }
    let mut entry = json!({ "name": name, "handle": handle, "source": source, "type": kind });
    if let Some(t) = targets.first() {
        entry["target"] = json!(t.target);
    }
    Ok(entry)
}

/// Map godaddy.toml extensions (embed / checkout / blocks) to the release
/// `uiExtensions` input. Mirrors the TS release mapping (single target each).
fn build_ui_extensions(config: &crate::config::Config) -> cli_engine::Result<Vec<Value>> {
    let mut out = Vec::new();
    let Some(exts) = &config.extensions else {
        return Ok(out);
    };
    for e in &exts.embed {
        out.push(ui_extension_entry(
            &e.name, &e.handle, &e.source, "embed", &e.targets,
        )?);
    }
    for e in &exts.checkout {
        out.push(ui_extension_entry(
            &e.name, &e.handle, &e.source, "checkout", &e.targets,
        )?);
    }
    if let Some(b) = &exts.blocks {
        // Blocks carries no name/handle/targets in config; use the same fixed
        // identifiers the TS release path uses.
        out.push(
            json!({ "name": "Blocks", "handle": "blocks", "source": b.source, "type": "blocks" }),
        );
    }
    Ok(out)
}

fn release_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_with_context(
        CommandSpec::new("release", "Create a new application release")
            .with_long(
                "Tag a new versioned release for a GoDaddy developer-platform application. \
                The version must follow semver (e.g. 1.2.3). A release is required before \
                running `application deploy`. Use `application info --name <name>` to \
                retrieve the application ID.",
            )
            .with_system("applications")
            .with_tier(Tier::Mutate)
            .with_scopes(&[APP_REGISTRY_READ, APP_REGISTRY_WRITE])
            .with_output_schema::<ApplicationRelease>()
            .with_arg(
                clap::Arg::new("application-id")
                    .long("application-id")
                    .value_name("ID")
                    .required(true)
                    .help("Application ID"),
            )
            .with_arg(
                clap::Arg::new("version")
                    .long("version")
                    .value_name("VERSION")
                    .required(true)
                    .help("Semver release version"),
            )
            .with_arg(
                clap::Arg::new("description")
                    .long("description")
                    .value_name("TEXT")
                    .help("Release description"),
            ),
        |ctx| async move {
            let app_id = arg_str(&ctx, "application-id").to_owned();
            let version = arg_str(&ctx, "version").to_owned();
            let description = ctx
                .args
                .get("description")
                .and_then(|v| v.as_str())
                .map(str::to_owned);
            let mut input = json!({ "applicationId": app_id, "version": version });
            if let Some(desc) = description {
                input["description"] = json!(desc);
            }

            // Include actions, webhook subscriptions, and UI extensions from
            // godaddy.toml so configured behavior is captured in the release.
            // Without this, everything added via `application add` was silently
            // dropped. A missing config is non-fatal (empty arrays); a config
            // with too many targets per extension is a hard error.
            let (actions, subscriptions, ui_extensions) = match crate::config::read_config(
                &crate::config::config_path(Some(&ctx.middleware.env)),
            ) {
                Ok(config) => {
                    let actions: Vec<Value> = config
                        .actions
                        .iter()
                        .map(|a| json!({ "name": a.name, "url": a.url }))
                        .collect();
                    let subscriptions: Vec<Value> = config
                        .subscriptions
                        .as_ref()
                        .map(|s| {
                            s.webhook
                                .iter()
                                .map(
                                    |w| json!({ "name": w.name, "events": w.events, "url": w.url }),
                                )
                                .collect()
                        })
                        .unwrap_or_default();
                    let ui_extensions = build_ui_extensions(&config)?;
                    (actions, subscriptions, ui_extensions)
                }
                Err(_) => (Vec::new(), Vec::new(), Vec::new()),
            };
            input["actions"] = json!(actions);
            input["subscriptions"] = json!(subscriptions);
            input["uiExtensions"] = json!(ui_extensions);

            let client = make_client(&ctx).await?;
            let data = client.create_release(input).await.map_err(client_err)?;
            // Release is keyed by `--application-id`, not name. Do not prefill
            // `name` from godaddy.toml — that manifest may belong to a different app.
            let name_param = NextActionParam::required();
            Ok(
                CommandResult::new(data["createRelease"].clone()).with_next_actions(vec![
                    next_action(
                        "application deploy --name <name>",
                        "Deploy the released application",
                    )
                    .with_param("name", name_param.clone()),
                    next_action(
                        "application info --name <name>",
                        "Inspect application and latest release",
                    )
                    .with_param("name", name_param),
                ]),
            )
        },
    )
}

fn deploy_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_streaming(
        CommandSpec::new("deploy", "Deploy an application, streaming progress events")
            .with_long(
                "Read godaddy.toml from the current directory, bundle all declared extensions \
                with esbuild, run the security scanner (rules SEC101–SEC115) on each bundle, \
                then upload the artifacts to the latest release of the named application. \
                Progress is streamed as JSON events. A release must exist before deploying; \
                create one with `application release`.",
            )
            .with_system("applications")
            .with_tier(Tier::Mutate)
            .with_scopes(&[APP_REGISTRY_READ, APP_REGISTRY_WRITE])
            .with_arg(
                clap::Arg::new("name")
                    .long("name")
                    .short('n')
                    .value_name("NAME")
                    .required(true)
                    .help("Application name"),
            ),
        |ctx, sender: StreamSender| async move {
            let name = ctx
                .args
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_owned();
            let env = ctx.middleware.env.clone();
            let token = tap_deploy_err(&sender, ctx.credential().await).await?.token;
            let base_url = api_url_for_env(&env);
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
                    cli_engine::CliCoreError::message(format!("config read failed: {e}"))
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
                    .map_err(client_err),
            )
            .await?;
            let app = &app_data["application"];
            if app.is_null() {
                let err =
                    cli_engine::CliCoreError::message(format!("application '{name}' not found"));
                return Err(fail_deploy(&sender, err).await);
            }
            let application_id = app["id"].as_str().unwrap_or("").to_owned();
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
                    "application '{name}' has no releases — create one first with: gddy application release --application-id {application_id} --version 0.0.1"
                ));
                return Err(fail_deploy(&sender, err).await);
            };
            sender
                .send(json!({ "type": "step", "name": "release.lookup", "status": "completed", "releaseId": release_id }))
                .await;

            // Process each extension
            let extensions = collect_extensions(&config);
            let total = extensions.len();
            sender
                .send(json!({ "type": "step", "name": "extensions", "status": "started", "total": total }))
                .await;

            for (i, ext) in extensions.iter().enumerate() {
                tap_deploy_err(
                    &sender,
                    deploy_extension(
                        &client,
                        &sender,
                        &application_id,
                        &release_id,
                        ext,
                        i + 1,
                        total,
                    )
                    .await,
                )
                .await?;
            }

            tap_deploy_err(
                &sender,
                finalize_deploy_activation(&client, &sender, &application_id, &release_id).await,
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

/// Finalize a deploy: activate the release, then promote the application to
/// `ACTIVE`. Deploy must activate the release before promoting the application.
async fn finalize_deploy_activation(
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
        .map_err(client_err)?;
    sender
        .send(json!({ "type": "step", "name": "release.activate", "status": "completed" }))
        .await;
    sender
        .send(json!({ "type": "step", "name": "application.activate", "status": "started" }))
        .await;
    client
        .update_application(application_id, json!({ "status": "ACTIVE" }))
        .await
        .map_err(client_err)?;
    sender
        .send(json!({ "type": "step", "name": "application.activate", "status": "completed" }))
        .await;

    Ok(())
}

/// Collect extension source files from config, returning (name, source_path, ext_type) triples.
/// One deployable extension: its name, source entry-point, type, and the
/// configured target surfaces (empty for blocks / untargeted extensions).
struct ExtensionDeploy {
    name: String,
    source: String,
    ext_type: crate::extension::ExtensionType,
    targets: Vec<String>,
}

fn collect_extensions(config: &crate::config::Config) -> Vec<ExtensionDeploy> {
    let mut result = Vec::new();
    if let Some(exts) = &config.extensions {
        for e in &exts.embed {
            result.push(ExtensionDeploy {
                name: e.name.clone(),
                source: e.source.clone(),
                ext_type: crate::extension::ExtensionType::Embed,
                targets: e.targets.iter().map(|t| t.target.clone()).collect(),
            });
        }
        for e in &exts.checkout {
            result.push(ExtensionDeploy {
                name: e.name.clone(),
                source: e.source.clone(),
                ext_type: crate::extension::ExtensionType::Checkout,
                targets: e.targets.iter().map(|t| t.target.clone()).collect(),
            });
        }
        if let Some(blocks) = &exts.blocks {
            result.push(ExtensionDeploy {
                name: "blocks".to_owned(),
                source: blocks.source.clone(),
                ext_type: crate::extension::ExtensionType::Blocks,
                targets: Vec::new(),
            });
        }
    }
    result
}

/// Resolve the upload target(s) for one extension. A blocks extension always
/// uploads to the `blocks` target; embed/checkout upload once per configured
/// target, or a single untargeted upload (`None`) when none are configured.
fn resolve_upload_targets(
    ext_type: crate::extension::ExtensionType,
    targets: &[String],
) -> Vec<Option<String>> {
    match ext_type {
        crate::extension::ExtensionType::Blocks => vec![Some("blocks".to_owned())],
        _ if targets.is_empty() => vec![None],
        _ => targets.iter().cloned().map(Some).collect(),
    }
}

fn upload_completed_event(
    extension_name: &str,
    target: &Option<String>,
    is_final_target: bool,
    index: usize,
    total: usize,
) -> Value {
    let mut event = json!({
        "type": "progress",
        "name": "extension.upload",
        "status": "completed",
        "extensionName": extension_name,
        "target": target,
    });
    if is_final_target {
        event["percent"] = json!(index * 100 / total.max(1));
    }
    event
}

/// Parse a comma-separated `--target` value into extension targets, trimming
/// whitespace and dropping empty entries.
fn parse_targets(raw: &str) -> Vec<crate::config::ExtensionTarget> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| crate::config::ExtensionTarget {
            target: s.to_owned(),
        })
        .collect()
}

async fn deploy_extension(
    client: &ApplicationClient,
    sender: &StreamSender,
    application_id: &str,
    release_id: &str,
    ext: &ExtensionDeploy,
    index: usize,
    total: usize,
) -> cli_engine::Result<()> {
    let ext_name = &ext.name;
    let source_path = &ext.source;
    let ext_type = ext.ext_type;

    // ---- Bundle ----
    sender
        .send(json!({
            "type": "progress",
            "name": "extension.bundle",
            "status": "started",
            "extensionName": ext_name,
            "percent": ((index - 1) * 100 / total.max(1)),
        }))
        .await;

    let source = std::path::Path::new(source_path);
    let ext_dir = source.parent().unwrap_or(std::path::Path::new("."));
    let bundle = crate::extension::bundle_extension(source, ext_type, ext_dir)
        .await
        .map_err(|e| {
            cli_engine::CliCoreError::message(format!("bundle failed for '{ext_name}': {e}"))
        })?;

    sender
        .send(json!({
            "type": "progress",
            "name": "extension.bundle",
            "status": "completed",
            "extensionName": ext_name,
            "sha256": bundle.sha256,
        }))
        .await;

    // ---- Security scan ----
    sender
        .send(json!({
            "type": "progress",
            "name": "extension.scan",
            "status": "started",
            "extensionName": ext_name,
        }))
        .await;

    let content = String::from_utf8_lossy(&bundle.bytes);
    let findings = crate::extension::scan_bundle(&content, source_path);

    if crate::extension::is_blocked(&findings) {
        let blocked_msgs: Vec<String> = findings
            .iter()
            .filter(|f| f.severity == crate::extension::Severity::Block)
            .map(|f| {
                if f.snippet.is_empty() {
                    format!("  {} ({}:{}): {}", f.rule_id, f.file, f.line, f.message)
                } else {
                    format!(
                        "  {} ({}:{}): {}\n    > {}",
                        f.rule_id, f.file, f.line, f.message, f.snippet
                    )
                }
            })
            .collect();
        return Err(cli_engine::CliCoreError::message(format!(
            "security scan blocked deployment of '{ext_name}':\n{}",
            blocked_msgs.join("\n")
        )));
    }

    sender
        .send(json!({
            "type": "progress",
            "name": "extension.scan",
            "status": "completed",
            "extensionName": ext_name,
            "findings": findings.len(),
        }))
        .await;

    let bytes = bytes::Bytes::from(bundle.bytes);

    // Upload the bundle once per configured target (blocks -> "blocks";
    // embed/checkout -> each target, or a single untargeted upload).
    let upload_targets = resolve_upload_targets(ext_type, &ext.targets);
    for (target_index, target) in upload_targets.iter().enumerate() {
        sender
            .send(json!({ "type": "progress", "name": "extension.upload", "status": "started", "extensionName": ext_name, "target": target }))
            .await;

        let mut upload_input = json!({
            "applicationId": application_id,
            "releaseId": release_id,
            "contentType": "JS",
        });
        if let Some(t) = target {
            upload_input["target"] = json!(t);
        }

        let upload_data = client
            .generate_upload_url(upload_input)
            .await
            .map_err(client_err)?;

        let upload = &upload_data["generateReleaseUploadUrl"];
        let upload_url = upload["url"].as_str().unwrap_or("").to_owned();
        let upload_id = upload["uploadId"].as_str().unwrap_or("").to_owned();
        let max_size_bytes = upload["maxSizeBytes"].as_u64();

        // Parse required headers from ["key:value"] array
        let mut headers = serde_json::Map::new();
        if let Some(arr) = upload["requiredHeaders"].as_array() {
            for h in arr {
                if let Some(s) = h.as_str()
                    && let Some((k, v)) = s.split_once(':')
                {
                    headers.insert(k.trim().to_owned(), json!(v.trim()));
                }
            }
        }

        client
            .upload_artifact(
                &upload_url,
                &upload_id,
                &json!(headers),
                max_size_bytes,
                bytes.clone(),
                UploadOptions::default(),
            )
            .await
            .map_err(client_err)?;

        sender
            .send(upload_completed_event(
                ext_name,
                target,
                target_index + 1 == upload_targets.len(),
                index,
                total,
            ))
            .await;
    }

    Ok(())
}

pub fn add_group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(
        GroupSpec::new("add", "Add components to an application").with_long(
            "Append actions, webhook subscriptions, or UI extensions to the \
                godaddy.toml manifest in the current directory. These commands do not \
                require authentication and do not contact the API; run \
                `application deploy` to publish the updated manifest.",
        ),
    )
    .with_command(RuntimeCommandSpec::new_with_context(
        CommandSpec::new("action", "Add an action to godaddy.toml")
            .with_long(
                "Append an action entry to the godaddy.toml manifest in the current \
                    directory. An action is an HTTP endpoint that the platform calls on \
                    behalf of the application; it is identified by a name and a public \
                    HTTPS URL. The manifest is updated in place; run \
                    `application validate <name>` to confirm remote application state.",
            )
            .with_system("applications")
            .with_tier(Tier::Mutate)
            .with_output_schema::<ConfigAction>()
            .no_auth(true)
            .with_arg(
                clap::Arg::new("name")
                    .long("name")
                    .required(true)
                    .help("Unique action name written into godaddy.toml"),
            )
            .with_arg(
                clap::Arg::new("url")
                    .long("url")
                    .required(true)
                    .help("Public HTTPS URL the platform will invoke for this action"),
            ),
        |ctx| async move {
            let name = arg_str(&ctx, "name").to_owned();
            let url = arg_str(&ctx, "url").to_owned();
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
                .with_next_actions(add_config_next_actions(&config.name)))
        },
    ))
    .with_command(RuntimeCommandSpec::new_with_context(
        CommandSpec::new("subscription", "Add a webhook subscription to godaddy.toml")
            .with_long(
                "Append a webhook subscription entry to the godaddy.toml manifest in \
                    the current directory. A subscription routes platform events to an HTTPS \
                    endpoint. Provide one or more event types with --events; run \
                    `gddy webhook events` to discover the full list of valid event types.",
            )
            .with_system("applications")
            .with_tier(Tier::Mutate)
            .with_output_schema::<ConfigSubscription>()
            .no_auth(true)
            .with_arg(
                clap::Arg::new("name")
                    .long("name")
                    .required(true)
                    .help("Unique subscription name written into godaddy.toml"),
            )
            .with_arg(
                clap::Arg::new("url")
                    .long("url")
                    .required(true)
                    .help("Public HTTPS URL that will receive webhook POST requests"),
            )
            .with_arg(
                clap::Arg::new("events")
                    .long("events")
                    .num_args(1..)
                    .value_name("EVENT")
                    .required(true)
                    .help(
                        "One or more event types to subscribe to \
                            (run `gddy webhook events` to list valid values)",
                    ),
            ),
        |ctx| async move {
            let name = arg_str(&ctx, "name").to_owned();
            let url = arg_str(&ctx, "url").to_owned();
            let events: Vec<String> = ctx
                .args
                .get("events")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default();
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
                    .with_next_actions(add_config_next_actions(&config.name)),
            )
        },
    ))
    .with_group(add_extension_group())
}

pub fn add_extension_group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(
        GroupSpec::new("extension", "Add an extension to godaddy.toml").with_long(
            "Append a UI extension entry to the godaddy.toml manifest in the current \
                directory. Extensions are JavaScript bundles that the platform renders \
                inside store surfaces. Three types are supported: embed (inline widget), \
                checkout (checkout-flow widget), and blocks (content blocks). Run \
                `application deploy` to bundle and upload the registered extensions.",
        ),
    )
    .with_command(RuntimeCommandSpec::new_with_context(
        CommandSpec::new("embed", "Add an embed extension")
            .with_long(
                "Register an embed UI extension in godaddy.toml. An embed extension is a \
                JavaScript bundle rendered as an inline widget inside a store surface. \
                Provide a unique name, a platform handle, and the path to the source \
                entry-point file. Run `application deploy` to bundle and upload.",
            )
            .with_system("applications")
            .with_tier(Tier::Mutate)
            .with_output_schema::<ExtensionHandle>()
            .no_auth(true)
            .with_arg(
                clap::Arg::new("name")
                    .long("name")
                    .required(true)
                    .help("Unique extension name written into godaddy.toml"),
            )
            .with_arg(
                clap::Arg::new("handle")
                    .long("handle")
                    .required(true)
                    .help("Platform handle used to identify this extension surface"),
            )
            .with_arg(
                clap::Arg::new("source")
                    .long("source")
                    .required(true)
                    .help("Path to the JavaScript entry-point file for this extension"),
            )
            .with_arg(
                clap::Arg::new("target")
                    .long("target")
                    .required(true)
                    .help("Comma-separated target surface(s) for this extension"),
            ),
        |ctx| async move {
            let name = arg_str(&ctx, "name").to_owned();
            let handle = arg_str(&ctx, "handle").to_owned();
            let source = arg_str(&ctx, "source").to_owned();
            let targets = parse_targets(arg_str(&ctx, "target"));
            if targets.is_empty() {
                return Err(cli_engine::CliCoreError::message(
                    "at least one --target is required (comma-separated)",
                ));
            }
            let path = crate::config::config_path(Some(&ctx.middleware.env));
            let mut config = crate::config::read_config(&path)
                .map_err(|e| cli_engine::CliCoreError::message(e.to_string()))?;
            let exts = config
                .extensions
                .get_or_insert_with(|| crate::config::ExtensionsConfig {
                    embed: vec![],
                    checkout: vec![],
                    blocks: None,
                });
            exts.embed.push(crate::config::EmbedExtensionConfig {
                name: name.clone(),
                handle: handle.clone(),
                source,
                targets,
            });
            crate::config::write_config(&path, &config)
                .map_err(|e| cli_engine::CliCoreError::message(e.to_string()))?;
            Ok(
                CommandResult::new(json!({ "name": name, "handle": handle, "type": "embed" }))
                    .with_next_actions(add_config_next_actions(&config.name)),
            )
        },
    ))
    .with_command(RuntimeCommandSpec::new_with_context(
        CommandSpec::new("checkout", "Add a checkout extension")
            .with_long(
                "Register a checkout UI extension in godaddy.toml. A checkout extension is \
                a JavaScript bundle rendered during the store checkout flow. Provide a \
                unique name, a platform handle, and the path to the source entry-point \
                file. Run `application deploy` to bundle and upload.",
            )
            .with_system("applications")
            .with_tier(Tier::Mutate)
            .with_output_schema::<ExtensionHandle>()
            .no_auth(true)
            .with_arg(
                clap::Arg::new("name")
                    .long("name")
                    .required(true)
                    .help("Unique extension name written into godaddy.toml"),
            )
            .with_arg(
                clap::Arg::new("handle")
                    .long("handle")
                    .required(true)
                    .help("Platform handle used to identify this extension surface"),
            )
            .with_arg(
                clap::Arg::new("source")
                    .long("source")
                    .required(true)
                    .help("Path to the JavaScript entry-point file for this extension"),
            )
            .with_arg(
                clap::Arg::new("target")
                    .long("target")
                    .required(true)
                    .help("Comma-separated target surface(s) for this extension"),
            ),
        |ctx| async move {
            let name = arg_str(&ctx, "name").to_owned();
            let handle = arg_str(&ctx, "handle").to_owned();
            let source = arg_str(&ctx, "source").to_owned();
            let targets = parse_targets(arg_str(&ctx, "target"));
            if targets.is_empty() {
                return Err(cli_engine::CliCoreError::message(
                    "at least one --target is required (comma-separated)",
                ));
            }
            let path = crate::config::config_path(Some(&ctx.middleware.env));
            let mut config = crate::config::read_config(&path)
                .map_err(|e| cli_engine::CliCoreError::message(e.to_string()))?;
            let exts = config
                .extensions
                .get_or_insert_with(|| crate::config::ExtensionsConfig {
                    embed: vec![],
                    checkout: vec![],
                    blocks: None,
                });
            exts.checkout.push(crate::config::CheckoutExtensionConfig {
                name: name.clone(),
                handle: handle.clone(),
                source,
                targets,
            });
            crate::config::write_config(&path, &config)
                .map_err(|e| cli_engine::CliCoreError::message(e.to_string()))?;
            Ok(
                CommandResult::new(json!({ "name": name, "handle": handle, "type": "checkout" }))
                    .with_next_actions(add_config_next_actions(&config.name)),
            )
        },
    ))
    .with_command(RuntimeCommandSpec::new_with_context(
        CommandSpec::new("blocks", "Add a blocks extension")
            .with_long(
                "Register a blocks UI extension in godaddy.toml. A blocks extension is a \
                JavaScript bundle that provides content blocks within a store. Only one \
                blocks extension is supported per application; running this command again \
                will overwrite the existing entry. Run `application deploy` to bundle and \
                upload.",
            )
            .with_system("applications")
            .with_tier(Tier::Mutate)
            .with_output_schema::<ExtensionBlocks>()
            .no_auth(true)
            .with_arg(
                clap::Arg::new("source")
                    .long("source")
                    .required(true)
                    .help("Path to the JavaScript entry-point file for the blocks extension"),
            ),
        |ctx| async move {
            let source = arg_str(&ctx, "source").to_owned();
            let path = crate::config::config_path(Some(&ctx.middleware.env));
            let mut config = crate::config::read_config(&path)
                .map_err(|e| cli_engine::CliCoreError::message(e.to_string()))?;
            let exts = config
                .extensions
                .get_or_insert_with(|| crate::config::ExtensionsConfig {
                    embed: vec![],
                    checkout: vec![],
                    blocks: None,
                });
            exts.blocks = Some(crate::config::BlocksExtensionConfig {
                source: source.clone(),
            });
            crate::config::write_config(&path, &config)
                .map_err(|e| cli_engine::CliCoreError::message(e.to_string()))?;
            Ok(
                CommandResult::new(json!({ "source": source, "type": "blocks" }))
                    .with_next_actions(add_config_next_actions(&config.name)),
            )
        },
    ))
}

#[cfg(test)]
mod tests {
    use cli_engine::{Cli, CliConfig, Stage};
    use serde_json::json;

    use super::{
        add_config_next_actions, deploy_next_actions, update_command, validate_command,
        validate_remote_application,
    };

    #[test]
    fn reused_next_action_helpers_have_expected_size() {
        assert_eq!(add_config_next_actions("app").len(), 2);
        assert_eq!(deploy_next_actions("app").len(), 3);
    }

    #[test]
    fn add_config_next_actions_skips_empty_name_prefill() {
        let actions = add_config_next_actions("");
        let name = &actions[0].params["name"];
        assert!(name.required);
        assert_eq!(name.value.as_deref(), None);
    }

    fn update_clap_command() -> clap::Command {
        update_command().spec.clap_command()
    }

    fn validate_clap_command() -> clap::Command {
        validate_command().spec.clap_command()
    }

    #[test]
    fn validate_requires_name() {
        let err = validate_clap_command()
            .try_get_matches_from(["validate"])
            .expect_err("validate without name should be rejected");
        assert_eq!(
            err.kind(),
            clap::error::ErrorKind::MissingRequiredArgument,
            "expected MissingRequiredArgument, got: {err}"
        );
    }

    #[test]
    fn validate_accepts_positional_name() {
        validate_clap_command()
            .try_get_matches_from(["validate", "my-app"])
            .expect("positional name should be accepted");
    }

    #[test]
    fn validate_remote_healthy_app_is_valid() {
        let (valid, errors, warnings) = validate_remote_application(&json!({
            "url": "https://example.com",
            "proxyUrl": "https://proxy.example.com",
            "status": "ACTIVE",
        }));
        assert!(valid);
        assert!(errors.is_empty());
        assert!(warnings.is_empty());
    }

    #[test]
    fn validate_remote_missing_url_is_error() {
        let (valid, errors, warnings) = validate_remote_application(&json!({
            "url": "",
            "proxyUrl": "https://proxy.example.com",
            "status": "ACTIVE",
        }));
        assert!(!valid);
        assert_eq!(errors, vec!["Application URL is required".to_owned()]);
        assert!(warnings.is_empty());
    }

    #[test]
    fn validate_remote_missing_proxy_and_inactive_are_warnings() {
        let (valid, errors, warnings) = validate_remote_application(&json!({
            "url": "https://example.com",
            "proxyUrl": null,
            "status": "INACTIVE",
        }));
        assert!(valid, "warnings alone should not invalidate");
        assert!(errors.is_empty());
        assert_eq!(
            warnings,
            vec![
                "Proxy URL is not set".to_owned(),
                "Application is currently inactive".to_owned(),
            ]
        );
    }

    #[test]
    fn status_rejects_values_outside_active_inactive() {
        for bad in ["active", "DISABLED", "PENDING"] {
            let err = update_clap_command()
                .try_get_matches_from(["update", "--id", "app-1", "--status", bad])
                .expect_err("invalid --status should be rejected");
            assert_eq!(
                err.kind(),
                clap::error::ErrorKind::InvalidValue,
                "--status {bad:?} should fail possible-value validation, got: {err}"
            );
        }
    }

    #[test]
    fn status_accepts_active_and_inactive() {
        for good in ["ACTIVE", "INACTIVE"] {
            update_clap_command()
                .try_get_matches_from(["update", "--id", "app-1", "--status", good])
                .expect("ACTIVE|INACTIVE --status should be accepted");
        }
    }

    #[test]
    fn update_requires_at_least_one_field() {
        let err = update_clap_command()
            .try_get_matches_from(["update", "--id", "app-1"])
            .expect_err("update with only --id should be rejected at parse time");
        assert_eq!(
            err.kind(),
            clap::error::ErrorKind::MissingRequiredArgument,
            "expected MissingRequiredArgument, got: {err}"
        );
    }

    #[test]
    fn update_accepts_any_single_field() {
        update_clap_command()
            .try_get_matches_from(["update", "--id", "app-1", "--label", "New"])
            .expect("--label alone should be accepted");
        update_clap_command()
            .try_get_matches_from(["update", "--id", "app-1", "--description", "Desc"])
            .expect("--description alone should be accepted");
        update_clap_command()
            .try_get_matches_from(["update", "--id", "app-1", "--status", "ACTIVE"])
            .expect("--status alone should be accepted");
    }

    #[test]
    fn update_accepts_multiple_fields() {
        update_clap_command()
            .try_get_matches_from([
                "update",
                "--id",
                "app-1",
                "--label",
                "New",
                "--description",
                "Desc",
                "--status",
                "INACTIVE",
            ])
            .expect("multiple update fields should be allowed together");
    }

    /// API commands must stay fail-closed: `application list` calls the backend,
    /// so it must require authentication. Built with **no auth provider
    /// registered**, the engine's default `AuthRequirement::Required` must reject
    /// it before the handler runs (no network call). This guards against someone
    /// mistakenly marking an API command `no_auth(true)`, which would let it run
    /// unauthenticated.
    #[tokio::test]
    async fn application_list_requires_auth() {
        let cli = Cli::new(
            CliConfig::new("gddy", "GoDaddy developer CLI", "gddy")
                .with_min_stage(Stage::Experimental)
                .with_default_auth_provider("godaddy")
                .with_module(super::super::module()),
        );

        let output = cli
            .run(["gddy", "application", "list", "--output", "json"])
            .await;

        // The engine maps auth-resolution failures to exit code 2; together with
        // the provider-named error below this proves the command was rejected at
        // credential resolution, not by the handler hitting the network.
        const AUTH_FAILURE_EXIT: i32 = 2;
        assert_eq!(
            output.exit_code, AUTH_FAILURE_EXIT,
            "application list must fail closed at auth resolution, got: {}",
            output.rendered
        );
        let json: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("valid json output");
        let message = json["error"]["message"].as_str().unwrap_or_default();
        assert!(
            message.contains("provider"),
            "expected an auth-provider resolution error, got: {}",
            output.rendered
        );
    }

    #[test]
    fn parse_targets_trims_splits_and_drops_empties() {
        let parsed = super::parse_targets(" a , b ,, c ");
        let got: Vec<String> = parsed.into_iter().map(|t| t.target).collect();
        assert_eq!(got, vec!["a", "b", "c"]);
        assert!(super::parse_targets("   ").is_empty());
        assert!(super::parse_targets("").is_empty());
    }

    #[test]
    fn resolve_upload_targets_by_type() {
        use crate::extension::ExtensionType;

        // Blocks always uploads to the fixed "blocks" target.
        assert_eq!(
            super::resolve_upload_targets(ExtensionType::Blocks, &[]),
            vec![Some("blocks".to_owned())]
        );
        // Embed/checkout with no targets: a single untargeted upload.
        assert_eq!(
            super::resolve_upload_targets(ExtensionType::Embed, &[]),
            vec![None]
        );
        // Embed/checkout with targets: one upload per target.
        assert_eq!(
            super::resolve_upload_targets(
                ExtensionType::Checkout,
                &["a".to_owned(), "b".to_owned()]
            ),
            vec![Some("a".to_owned()), Some("b".to_owned())]
        );
    }

    #[test]
    fn final_target_completion_preserves_legacy_upload_event() {
        let target = Some("admin.product.detail".to_owned());

        let event = super::upload_completed_event("widget", &target, true, 1, 1);

        assert_eq!(
            event,
            serde_json::json!({
                "type": "progress",
                "name": "extension.upload",
                "status": "completed",
                "extensionName": "widget",
                "target": "admin.product.detail",
                "percent": 100,
            })
        );
    }

    #[test]
    fn ui_extension_entry_maps_fields_and_target() {
        use crate::config::ExtensionTarget;

        // No targets: `target` is omitted.
        let none = super::ui_extension_entry("Widget", "widget", "src/w.ts", "embed", &[])
            .expect("entry builds");
        assert_eq!(none["name"], "Widget");
        assert_eq!(none["handle"], "widget");
        assert_eq!(none["type"], "embed");
        assert_eq!(none["source"], "src/w.ts");
        assert!(none.get("target").is_none());

        // Exactly one target: `target` is set to that value.
        let one = super::ui_extension_entry(
            "Widget",
            "widget",
            "src/w.ts",
            "checkout",
            &[ExtensionTarget {
                target: "checkout.block".to_owned(),
            }],
        )
        .expect("entry builds");
        assert_eq!(one["target"], "checkout.block");
    }

    #[test]
    fn ui_extension_entry_rejects_multiple_targets() {
        use crate::config::ExtensionTarget;
        let targets = vec![
            ExtensionTarget {
                target: "a".to_owned(),
            },
            ExtensionTarget {
                target: "b".to_owned(),
            },
        ];
        let err = super::ui_extension_entry("Widget", "widget", "src/w.ts", "embed", &targets)
            .expect_err("more than one target must be rejected");
        assert!(
            err.to_string().contains("only one target is supported"),
            "unexpected error: {err}"
        );
    }

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
        assert_eq!(event["result"]["status"], "ACTIVE");
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
                .contains("application enable"),
            "first next action should enable on a store: {event}"
        );
    }
}
