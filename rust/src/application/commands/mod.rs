use cli_engine::{
    CommandResult, CommandSpec, GroupSpec, NextAction, NextActionParam, RuntimeCommandSpec,
    RuntimeGroupSpec, StreamSender, Tier,
};
use serde_json::json;

use crate::application::client::{ApplicationClient, api_url_for_env};

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

fn arg_str<'a>(ctx: &'a cli_engine::CommandContext, key: &str) -> &'a str {
    ctx.args.get(key).and_then(|v| v.as_str()).unwrap_or("")
}

pub fn application_group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(
        GroupSpec::new("application", "Manage GoDaddy applications").with_alias("app"),
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
            .with_system("applications")
            .with_tier(Tier::Read)
            .with_default_fields("name,label,status"),
        |ctx| async move {
            let client = make_client(&ctx).await?;
            let data = client.list_applications().await.map_err(client_err)?;
            Ok(CommandResult::new(data).with_next_actions(vec![
                NextAction::new(
                    "application info --name <name>",
                    "Get details for a specific application",
                )
                .with_param("name", NextActionParam::required()),
                NextAction::new(
                    "application init --name <name> --description <desc> --url <url>",
                    "Initialize a new application",
                ),
            ]))
        },
    )
}

fn info_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_with_context(
        CommandSpec::new("info", "Get application details")
            .with_system("applications")
            .with_tier(Tier::Read)
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
            Ok(
                CommandResult::new(data["application"].clone()).with_next_actions(vec![
                    NextAction::new(
                        "application release --application-id <id> --version <ver>",
                        "Create a release",
                    ),
                    NextAction::new(
                        format!("application deploy --name {name}"),
                        "Deploy this application",
                    )
                    .with_param(
                        "name",
                        NextActionParam {
                            value: Some(name.clone()),
                            required: true,
                            ..Default::default()
                        },
                    ),
                    NextAction::new(
                        "application update --id <id>",
                        "Update application metadata",
                    ),
                ]),
            )
        },
    )
}

fn init_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_with_context(
        CommandSpec::new("init", "Create and initialize a new application")
            .with_system("applications")
            .with_tier(Tier::Mutate)
            .with_arg(
                clap::Arg::new("name")
                    .long("name")
                    .short('n')
                    .value_name("NAME")
                    .required(true)
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
                    .required(true)
                    .help("Application description"),
            )
            .with_arg(
                clap::Arg::new("url")
                    .long("url")
                    .value_name("URL")
                    .required(true)
                    .help("Application URL (must be public HTTPS)"),
            )
            .with_arg(
                clap::Arg::new("proxy-url")
                    .long("proxy-url")
                    .value_name("URL")
                    .help("Proxy URL (defaults to url)"),
            )
            .with_arg(
                clap::Arg::new("scopes")
                    .long("scopes")
                    .value_name("SCOPES")
                    .help("Comma-separated authorization scopes"),
            ),
        |ctx| async move {
            let name = arg_str(&ctx, "name").to_owned();
            let label = ctx
                .args
                .get("label")
                .and_then(|v| v.as_str())
                .unwrap_or(&name)
                .to_owned();
            let description = arg_str(&ctx, "description").to_owned();
            let url = arg_str(&ctx, "url").to_owned();
            let proxy_url = ctx
                .args
                .get("proxy-url")
                .and_then(|v| v.as_str())
                .unwrap_or(&url)
                .to_owned();
            let scopes: Vec<String> = ctx
                .args
                .get("scopes")
                .and_then(|v| v.as_str())
                .map(|s| s.split(',').map(|p| p.trim().to_owned()).collect())
                .unwrap_or_else(|| vec!["apps.app-registry:read".to_owned()]);

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
            let client_id = app["clientId"].as_str().unwrap_or("").to_owned();

            // Write godaddy.toml for the created application
            let config_path = crate::config::config_path(Some(ctx.middleware.env.as_str()));
            let config = crate::config::Config {
                name: name.clone(),
                client_id: client_id.clone(),
                description: Some(description.clone()),
                version: "0.0.0".to_owned(),
                url: url.clone(),
                proxy_url,
                authorization_scopes: scopes,
                actions: vec![],
                subscriptions: None,
                dependencies: vec![],
                extensions: None,
            };
            crate::config::write_config(&config_path, &config)
                .map_err(|e| cli_engine::CliCoreError::message(e.to_string()))?;

            Ok(CommandResult::new(app.clone()).with_next_actions(vec![
                NextAction::new(
                    "application add action --name <name> --url <url>",
                    "Add an action to godaddy.toml",
                ),
                NextAction::new(
                    "application validate",
                    "Validate the generated godaddy.toml",
                ),
                NextAction::new(
                    "application release --application-id <id> --version 0.0.1",
                    "Create the first release",
                ),
            ]))
        },
    )
}

fn validate_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_with_context(
        CommandSpec::new("validate", "Validate godaddy.toml config")
            .with_system("applications")
            .with_tier(Tier::Read)
            .no_auth(true)
            .with_arg(
                clap::Arg::new("config")
                    .long("config")
                    .value_name("PATH")
                    .help("Path to godaddy.toml (defaults to ./godaddy.toml)"),
            ),
        |ctx| async move {
            let path_str = ctx
                .args
                .get("config")
                .and_then(|v| v.as_str())
                .map(|s| s.to_owned());
            let path = path_str
                .as_deref()
                .map(std::path::Path::new)
                .map(std::path::Path::to_owned)
                .unwrap_or_else(|| crate::config::config_path(Some(ctx.middleware.env.as_str())));
            crate::config::read_config(&path)
                .map_err(|e| cli_engine::CliCoreError::message(format!("invalid config: {e}")))?;
            Ok(CommandResult::new(
                json!({ "valid": true, "path": path.display().to_string() }),
            ))
        },
    )
}

fn update_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_with_context(
        CommandSpec::new("update", "Update an application")
            .with_system("applications")
            .with_tier(Tier::Mutate)
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
                    .help("New label"),
            )
            .with_arg(
                clap::Arg::new("description")
                    .long("description")
                    .value_name("TEXT")
                    .help("New description"),
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
                    NextAction::new(
                        format!("application info --name {name}"),
                        "Inspect updated application",
                    )
                    .with_param(
                        "name",
                        NextActionParam {
                            value: Some(name.clone()),
                            required: true,
                            ..Default::default()
                        },
                    ),
                    NextAction::new(
                        format!("application deploy --name {name}"),
                        "Deploy updated application",
                    )
                    .with_param(
                        "name",
                        NextActionParam {
                            value: Some(name),
                            required: true,
                            ..Default::default()
                        },
                    ),
                ]),
            )
        },
    )
}

fn enable_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_with_context(
        CommandSpec::new("enable", "Enable an application on a store")
            .with_system("applications")
            .with_tier(Tier::Mutate)
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
                    NextAction::new(
                        format!("application disable {name} --store-id {store_id}"),
                        "Disable the application on the same store",
                    )
                    .with_param(
                        "name",
                        NextActionParam {
                            value: Some(name.clone()),
                            required: true,
                            ..Default::default()
                        },
                    )
                    .with_param(
                        "store-id",
                        NextActionParam {
                            value: Some(store_id.clone()),
                            required: true,
                            ..Default::default()
                        },
                    ),
                    NextAction::new(
                        format!("application info --name {name}"),
                        "Inspect application",
                    )
                    .with_param(
                        "name",
                        NextActionParam {
                            value: Some(name),
                            required: true,
                            ..Default::default()
                        },
                    ),
                ]),
            )
        },
    )
}

fn disable_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_with_context(
        CommandSpec::new("disable", "Disable an application on a store")
            .with_system("applications")
            .with_tier(Tier::Mutate)
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
                        NextAction::new(
                            format!("application enable {name} --store-id {store_id}"),
                            "Re-enable the application on the same store",
                        )
                        .with_param(
                            "name",
                            NextActionParam {
                                value: Some(name.clone()),
                                required: true,
                                ..Default::default()
                            },
                        )
                        .with_param(
                            "store-id",
                            NextActionParam {
                                value: Some(store_id.clone()),
                                required: true,
                                ..Default::default()
                            },
                        ),
                        NextAction::new(
                            format!("application info --name {name}"),
                            "Inspect application",
                        )
                        .with_param(
                            "name",
                            NextActionParam {
                                value: Some(name),
                                required: true,
                                ..Default::default()
                            },
                        ),
                    ],
                ),
            )
        },
    )
}

fn archive_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_with_context(
        CommandSpec::new("archive", "Archive an application")
            .with_system("applications")
            .with_tier(Tier::Destructive)
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
                    NextAction::new("application list", "List remaining applications"),
                ]),
            )
        },
    )
}

fn release_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_with_context(
        CommandSpec::new("release", "Create a new application release")
            .with_system("applications")
            .with_tier(Tier::Mutate)
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
            let client = make_client(&ctx).await?;
            let data = client.create_release(input).await.map_err(client_err)?;
            Ok(
                CommandResult::new(data["createRelease"].clone()).with_next_actions(vec![
                    NextAction::new("application deploy --name <name>", "Deploy this release"),
                    NextAction::new("application info --name <name>", "Inspect application"),
                ]),
            )
        },
    )
}

fn deploy_command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_streaming(
        CommandSpec::new("deploy", "Deploy an application, streaming progress events")
            .with_system("applications")
            .with_tier(Tier::Mutate)
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
            let token = ctx.credential().await?.token;
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
            let config = crate::config::read_config(&config_path).map_err(|e| {
                cli_engine::CliCoreError::message(format!("config read failed: {e}"))
            })?;
            sender
                .send(json!({ "type": "step", "name": "config.read", "status": "completed", "path": config_path.display().to_string() }))
                .await;

            // Look up the application and its latest release
            sender
                .send(json!({ "type": "step", "name": "application.lookup", "status": "started" }))
                .await;
            let app_data = client
                .get_application_with_releases(&name)
                .await
                .map_err(client_err)?;
            let app = &app_data["application"];
            if app.is_null() {
                return Err(cli_engine::CliCoreError::message(format!(
                    "application '{name}' not found"
                )));
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
                return Err(cli_engine::CliCoreError::message(format!(
                    "application '{name}' has no releases — create one first with: gddy application release --application-id {application_id} --version 0.0.1"
                )));
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
                deploy_extension(
                    &client,
                    &sender,
                    &application_id,
                    &release_id,
                    ext,
                    i + 1,
                    total,
                )
                .await?;
            }

            sender
                .send(json!({ "type": "step", "name": "deploy", "status": "completed", "name": name, "extensions": total }))
                .await;
            Ok(())
        },
    )
}

/// Collect extension source files from config, returning (name, source_path, ext_type) triples.
fn collect_extensions(
    config: &crate::config::Config,
) -> Vec<(String, String, crate::extension::ExtensionType)> {
    let mut result = Vec::new();
    if let Some(exts) = &config.extensions {
        for e in &exts.embed {
            result.push((
                e.name.clone(),
                e.source.clone(),
                crate::extension::ExtensionType::Embed,
            ));
        }
        for e in &exts.checkout {
            result.push((
                e.name.clone(),
                e.source.clone(),
                crate::extension::ExtensionType::Checkout,
            ));
        }
        if let Some(blocks) = &exts.blocks {
            result.push((
                "blocks".to_owned(),
                blocks.source.clone(),
                crate::extension::ExtensionType::Blocks,
            ));
        }
    }
    result
}

async fn deploy_extension(
    client: &ApplicationClient,
    sender: &StreamSender,
    application_id: &str,
    release_id: &str,
    (ext_name, source_path, ext_type): &(String, String, crate::extension::ExtensionType),
    index: usize,
    total: usize,
) -> cli_engine::Result<()> {
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
    let bundle = crate::extension::bundle_extension(source, *ext_type, ext_dir)
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

    let bytes = bundle.bytes;

    // Get presigned upload URL
    sender
        .send(json!({ "type": "progress", "name": "extension.upload", "status": "started", "extensionName": ext_name }))
        .await;

    let upload_data = client
        .generate_upload_url(json!({
            "applicationId": application_id,
            "releaseId": release_id,
            "contentType": "JS",
            "target": ext_name,
        }))
        .await
        .map_err(client_err)?;

    let upload = &upload_data["generateReleaseUploadUrl"];
    let upload_url = upload["url"].as_str().unwrap_or("").to_owned();

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
        .upload_artifact(&upload_url, &json!(headers), bytes)
        .await
        .map_err(client_err)?;

    sender
        .send(json!({
            "type": "progress",
            "name": "extension.upload",
            "status": "completed",
            "extensionName": ext_name,
            "percent": (index * 100 / total.max(1)),
        }))
        .await;

    Ok(())
}

pub fn add_group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(GroupSpec::new("add", "Add components to an application"))
        .with_command(RuntimeCommandSpec::new_with_context(
            CommandSpec::new("action", "Add an action to godaddy.toml")
                .with_system("applications")
                .with_tier(Tier::Mutate)
                .no_auth(true)
                .with_arg(
                    clap::Arg::new("name")
                        .long("name")
                        .required(true)
                        .help("Action name"),
                )
                .with_arg(
                    clap::Arg::new("url")
                        .long("url")
                        .required(true)
                        .help("Action URL"),
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
                Ok(CommandResult::new(json!({ "name": name, "url": url })))
            },
        ))
        .with_command(RuntimeCommandSpec::new_with_context(
            CommandSpec::new("subscription", "Add a webhook subscription to godaddy.toml")
                .with_system("applications")
                .with_tier(Tier::Mutate)
                .no_auth(true)
                .with_arg(
                    clap::Arg::new("name")
                        .long("name")
                        .required(true)
                        .help("Subscription name"),
                )
                .with_arg(
                    clap::Arg::new("url")
                        .long("url")
                        .required(true)
                        .help("Webhook URL"),
                )
                .with_arg(
                    clap::Arg::new("events")
                        .long("events")
                        .num_args(1..)
                        .value_name("EVENT")
                        .required(true)
                        .help("Webhook event types"),
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
                Ok(CommandResult::new(
                    json!({ "name": name, "url": url, "events": events }),
                ))
            },
        ))
        .with_group(add_extension_group())
}

pub fn add_extension_group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(GroupSpec::new(
        "extension",
        "Add an extension to godaddy.toml",
    ))
    .with_command(RuntimeCommandSpec::new_with_context(
        CommandSpec::new("embed", "Add an embed extension")
            .with_system("applications")
            .with_tier(Tier::Mutate)
            .no_auth(true)
            .with_arg(
                clap::Arg::new("name")
                    .long("name")
                    .required(true)
                    .help("Extension name"),
            )
            .with_arg(
                clap::Arg::new("handle")
                    .long("handle")
                    .required(true)
                    .help("Unique handle"),
            )
            .with_arg(
                clap::Arg::new("source")
                    .long("source")
                    .required(true)
                    .help("Source file path"),
            ),
        |ctx| async move {
            let name = arg_str(&ctx, "name").to_owned();
            let handle = arg_str(&ctx, "handle").to_owned();
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
            exts.embed.push(crate::config::EmbedExtensionConfig {
                name: name.clone(),
                handle: handle.clone(),
                source,
                targets: vec![],
            });
            crate::config::write_config(&path, &config)
                .map_err(|e| cli_engine::CliCoreError::message(e.to_string()))?;
            Ok(CommandResult::new(
                json!({ "name": name, "handle": handle, "type": "embed" }),
            ))
        },
    ))
    .with_command(RuntimeCommandSpec::new_with_context(
        CommandSpec::new("checkout", "Add a checkout extension")
            .with_system("applications")
            .with_tier(Tier::Mutate)
            .no_auth(true)
            .with_arg(
                clap::Arg::new("name")
                    .long("name")
                    .required(true)
                    .help("Extension name"),
            )
            .with_arg(
                clap::Arg::new("handle")
                    .long("handle")
                    .required(true)
                    .help("Unique handle"),
            )
            .with_arg(
                clap::Arg::new("source")
                    .long("source")
                    .required(true)
                    .help("Source file path"),
            ),
        |ctx| async move {
            let name = arg_str(&ctx, "name").to_owned();
            let handle = arg_str(&ctx, "handle").to_owned();
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
            exts.checkout.push(crate::config::CheckoutExtensionConfig {
                name: name.clone(),
                handle: handle.clone(),
                source,
                targets: vec![],
            });
            crate::config::write_config(&path, &config)
                .map_err(|e| cli_engine::CliCoreError::message(e.to_string()))?;
            Ok(CommandResult::new(
                json!({ "name": name, "handle": handle, "type": "checkout" }),
            ))
        },
    ))
    .with_command(RuntimeCommandSpec::new_with_context(
        CommandSpec::new("blocks", "Add a blocks extension")
            .with_system("applications")
            .with_tier(Tier::Mutate)
            .no_auth(true)
            .with_arg(
                clap::Arg::new("source")
                    .long("source")
                    .required(true)
                    .help("Source file path"),
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
            Ok(CommandResult::new(
                json!({ "source": source, "type": "blocks" }),
            ))
        },
    ))
}

#[cfg(test)]
mod tests {
    use cli_engine::{Cli, CliConfig};

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
                .with_default_auth_provider("godaddy")
                .with_module(super::super::module()),
        );

        let output = cli
            .run(["gddy", "application", "list", "--output", "json"])
            .await;

        // Exit code 2 is the engine's auth-failure code, and the rendered error
        // names the missing provider — proving the command was rejected at
        // credential resolution, not by the handler hitting the network.
        assert_eq!(
            output.exit_code, 2,
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
}
