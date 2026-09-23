//! `gddy platform app import` — pull an existing application's remote
//! config and webhook subscriptions into a local godaddy.toml.

use cli_engine::{CommandResult, CommandSpec, RuntimeCommandSpec, TableColumn, Tier};
use serde_json::{Value, json};

use super::schemas::ApplicationImport;
use crate::next_action::{next_action, required_value};
use crate::scopes::APP_REGISTRY_READ;

#[derive(Debug, Clone, clap::Args)]
struct ImportArgs {
    /// Name of the already-registered application to import.
    #[arg(value_name = "NAME")]
    name: String,

    /// Skip safety checks when the local godaddy.toml would be overwritten:
    /// allows discarding unpublished webhook subscription changes, and
    /// allows overwriting a manifest that belongs to a different application.
    #[arg(long)]
    force: bool,
}

/// The `releases(first: 1, orderBy: { createdAt: DESC })` node selected by
/// `ApplicationClient::get_application_with_releases` e.g. the
/// application's latest release, if it has one.
fn latest_release(app: &Value) -> Option<&Value> {
    app["releases"]["edges"]
        .as_array()?
        .first()
        .map(|edge| &edge["node"])
}

/// Maps the latest release's `subscriptions` into local `SubscriptionConfig`
/// entries, relativizing each webhook URL against `proxy_url` so it matches
/// the `/webhooks/...` shape hand-authored entries use.
fn subscriptions_from_latest_release(
    app: &Value,
    proxy_url: &str,
) -> Vec<crate::config::SubscriptionConfig> {
    latest_release(app)
        .and_then(|node| node["subscriptions"].as_array())
        .map(|subs| {
            subs.iter()
                .map(|sub| crate::config::SubscriptionConfig {
                    name: sub["name"].as_str().unwrap_or("").to_owned(),
                    events: sub["events"]
                        .as_array()
                        .map(|events| {
                            events
                                .iter()
                                .filter_map(|e| e.as_str().map(str::to_owned))
                                .collect()
                        })
                        .unwrap_or_default(),
                    url: crate::config::relativize_webhook_url(
                        sub["url"].as_str().unwrap_or(""),
                        proxy_url,
                    ),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// A subscription's `(name, url, events)` reduced to a comparable signature,
/// with `events` order-normalized. So two lists that differ only in
/// subscription/event ordering aren't reported as diverging.
fn subscription_signature(sub: &crate::config::SubscriptionConfig) -> String {
    let mut events = sub.events.clone();
    events.sort();
    format!("{}\u{0}{}\u{0}{}", sub.name, sub.url, events.join(","))
}

/// Names of `local` subscriptions with no identical counterpart in `remote`
/// e.g. local edits (via `add subscription`) that were never published
/// with `release`, and that an `import` pull is about to discard by
/// replacing `subscriptions.webhook` with the latest release's list.
fn subscriptions_at_risk(
    local: &[crate::config::SubscriptionConfig],
    remote: &[crate::config::SubscriptionConfig],
) -> Vec<String> {
    let remote_signatures: std::collections::BTreeSet<String> =
        remote.iter().map(subscription_signature).collect();
    local
        .iter()
        .filter(|sub| !remote_signatures.contains(&subscription_signature(sub)))
        .map(|sub| sub.name.clone())
        .collect()
}

/// Gate for overwriting local webhook subscriptions that aren't in the
/// application's latest published release.
fn confirm_overwrite_or_abort(
    ctx: &cli_engine::CommandContext,
    name: &str,
    at_risk: &[String],
) -> cli_engine::Result<()> {
    let subscriptions = at_risk.join(", ");
    if ctx.is_interactive() {
        let message = format!(
            "Local subscriptions.webhook has unpublished changes not present in \
             {name}'s latest release and will be lost: {subscriptions}. Overwrite \
             local godaddy.toml anyway?"
        );
        if cli_engine::prompt::prompt_confirm(&message, false)? {
            return Ok(());
        }
        return Err(cli_engine::CliCoreError::message(
            "aborted: local webhook subscription changes were not overwritten",
        ));
    }
    Err(cli_engine::CliCoreError::message(format!(
        "local subscriptions.webhook has unpublished changes not present in {name}'s \
         latest release and would be overwritten: {subscriptions}. Run `gddy platform app \
         release` first to publish them, or re-run with --force to discard them."
    )))
}

/// `filesWritten` is a small path-by-kind object (`config`), so it renders
/// as an indented property bag instead of a raw JSON dump.
fn import_view_columns() -> Vec<TableColumn> {
    vec![
        TableColumn::new("id", "ID"),
        TableColumn::new("name", "Name"),
        TableColumn::new("status", "Status"),
        TableColumn::new("clientId", "Client ID"),
        TableColumn::new("url", "URL").no_truncate(true),
        TableColumn::new("proxyUrl", "Proxy URL").no_truncate(true),
        TableColumn::new("authorizationScopes", "Authorization Scopes"),
        TableColumn::new("filesWritten", "Files Written")
            .nested(vec![TableColumn::new("config", "Config").no_truncate(true)]),
    ]
}

/// Reads the local manifest at `config_path`, if one exists. Only a missing
/// file is treated as "nothing to preserve" — a manifest that exists but
/// fails to parse/validate/read is a fatal error, since silently ignoring it
/// would let this command overwrite an unreadable manifest and discard
/// whatever locally-authored sections it held.
fn read_existing_config(
    config_path: &std::path::Path,
) -> cli_engine::Result<Option<crate::config::Config>> {
    match crate::config::read_config(config_path) {
        Ok(cfg) => Ok(Some(cfg)),
        Err(crate::config::ConfigError::NotFound { .. }) => Ok(None),
        Err(e) => Err(crate::error::GddyError::config(format!(
            "failed to read existing config at {}: {e}",
            config_path.display()
        ))
        .into_cli_error()),
    }
}

/// Guards against carrying a *different* application's locally-authored
/// sections (actions, dependencies, extensions, settings, version) into the
/// application being imported, e.g. running `import b` in a directory whose
/// `godaddy.toml` still describes application `a`. Returns `None` when there
/// is nothing to preserve (no existing manifest, or one that's confirmed to
/// belong to `name`).
fn existing_config_for(
    existing: Option<crate::config::Config>,
    name: &str,
    force: bool,
) -> cli_engine::Result<Option<crate::config::Config>> {
    let Some(cfg) = existing else {
        return Ok(None);
    };
    if cfg.name == name {
        return Ok(Some(cfg));
    }
    if !force {
        return Err(cli_engine::CliCoreError::message(format!(
            "godaddy.toml in this directory belongs to application '{}', not '{name}'. Re-run \
             in a directory with '{name}''s manifest (or none), or pass --force to overwrite it \
             and discard '{}'s locally-authored sections (actions, dependencies, extensions, \
             settings).",
            cfg.name, cfg.name
        )));
    }
    tracing::warn!(
        existing_app = %cfg.name,
        importing_app = %name,
        "existing godaddy.toml belongs to a different application; discarding its \
         locally-authored sections"
    );
    Ok(None)
}

/// Field overrides accepted only by `init --from-existing`'s deprecated
/// forwarding alias, for scripts written against v0.2.14's `--from-existing`
/// (which let `--description`/`--url`/`--proxy-url`/`--scopes` override the
/// fetched value). The `import` command itself has no override flags and
/// always passes `ImportOverrides::default()`.
#[derive(Debug, Clone, Default)]
pub(super) struct ImportOverrides {
    pub description: Option<String>,
    pub url: Option<String>,
    pub proxy_url: Option<String>,
    pub scopes: Option<String>,
}

/// Shared by the `import` command and `init --from-existing`'s deprecated
/// forwarding alias. Read-only against the API (no `createApplication`, no
/// `.env` write); safe to re-run to re-sync webhook subscriptions after a
/// new release.
pub(super) async fn run(
    ctx: &cli_engine::CommandContext,
    name: String,
    force: bool,
    overrides: ImportOverrides,
) -> cli_engine::Result<CommandResult> {
    let env = ctx.middleware.env.clone();
    let config_path = crate::config::config_path(Some(&env));

    let client = super::make_client(ctx).await?;
    let data = client
        .get_application_with_releases(&name)
        .await
        .map_err(super::client_err)?;
    let app = &data["application"];
    if app.is_null() {
        return Err(
            crate::error::GddyError::not_found(format!("application '{name}' not found"))
                .into_cli_error(),
        );
    }

    let client_id = app["clientId"].as_str().unwrap_or("").to_owned();
    let description = overrides
        .description
        .or_else(|| app["description"].as_str().map(str::to_owned))
        .unwrap_or_default();
    let url = overrides
        .url
        .or_else(|| app["url"].as_str().map(str::to_owned))
        .unwrap_or_default();
    let proxy_url = overrides
        .proxy_url
        .or_else(|| app["proxyUrl"].as_str().map(str::to_owned))
        .unwrap_or_default();
    let scopes: Vec<String> = overrides
        .scopes
        .map(|s| {
            s.split(',')
                .map(|p| p.trim())
                .filter(|p| !p.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .or_else(|| {
            app["authorizationScopes"].as_array().map(|scopes| {
                scopes
                    .iter()
                    .filter_map(|s| s.as_str().map(str::to_owned))
                    .collect()
            })
        })
        .unwrap_or_default();

    for (field, u) in [("url", &url), ("proxyUrl", &proxy_url)] {
        if !crate::platform::app::public_url::is_public_routable_url(u) {
            return Err(super::validation_err(format!(
                "Invalid application configuration: {field} must be a publicly-resolvable \
                 http(s) URL (localhost, loopback, and private IPs are not allowed)"
            )));
        }
    }

    let webhook_subscriptions = subscriptions_from_latest_release(app, &proxy_url);

    // Preserve locally-authored fields the API doesn't track (actions,
    // dependencies, extensions, settings), if a godaddy.toml for *this*
    // application already exists; this command only syncs identity, version,
    // and webhook subscriptions, not the whole manifest.
    let existing = read_existing_config(&config_path)?;
    let existing = existing_config_for(existing, &name, force)?;

    if let Some(existing_cfg) = &existing {
        let local_webhooks = existing_cfg
            .subscriptions
            .as_ref()
            .map(|s| s.webhook.as_slice())
            .unwrap_or_default();
        let at_risk = subscriptions_at_risk(local_webhooks, &webhook_subscriptions);
        if !at_risk.is_empty() && !force {
            confirm_overwrite_or_abort(ctx, &name, &at_risk)?;
        }
    }

    // Get latest release version from app; fall back to the local manifest's
    // version (e.g. an app with no release yet), then to a fresh-manifest default.
    let version = latest_release(app)
        .and_then(|node| node["version"].as_str())
        .map(str::to_owned)
        .or_else(|| existing.as_ref().map(|c| c.version.clone()))
        .unwrap_or_else(|| "0.0.0".to_owned());
    let actions = existing
        .as_ref()
        .map(|c| c.actions.clone())
        .unwrap_or_default();
    let dependencies = existing
        .as_ref()
        .map(|c| c.dependencies.clone())
        .unwrap_or_default();
    let settings = existing
        .as_ref()
        .map(|c| c.settings.clone())
        .unwrap_or_default();
    let extensions = existing.and_then(|c| c.extensions);

    let config = crate::config::Config {
        name: name.clone(),
        client_id,
        description: Some(description),
        version,
        url: url.clone(),
        proxy_url: proxy_url.clone(),
        authorization_scopes: scopes.clone(),
        actions,
        subscriptions: Some(crate::config::SubscriptionsConfig {
            webhook: webhook_subscriptions.clone(),
        }),
        dependencies,
        extensions,
        settings,
    };

    crate::config::write_config(&config_path, &config).map_err(|e| {
        crate::error::GddyError::config(format!("failed to write config: {e}")).into_cli_error()
    })?;

    let cwd = std::env::current_dir().unwrap_or_default();
    let subscriptions_json: Vec<_> = webhook_subscriptions
        .iter()
        .map(|s| json!({ "name": s.name, "url": s.url, "events": s.events }))
        .collect();

    Ok(CommandResult::new(json!({
        "id": app["id"].as_str().unwrap_or("").to_owned(),
        "name": name,
        "status": app["status"].as_str().unwrap_or("").to_owned(),
        "clientId": config.client_id,
        "url": url,
        "proxyUrl": proxy_url,
        "authorizationScopes": scopes,
        "subscriptions": subscriptions_json,
        "filesWritten": {
            "config": cwd.join(&config_path).display().to_string(),
        },
    }))
    .with_next_actions(vec![
        next_action(
            "platform app validate <name>",
            "Validate the remote application state",
        )
        .with_param("name", required_value(&name)),
        next_action(
            "platform app info --name <name>",
            "Inspect application details",
        )
        .with_param("name", required_value(&name)),
    ]))
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<ImportArgs, _, _, _>(
        CommandSpec::from_args::<ImportArgs>(
            "import",
            "Import an existing application's config and webhook subscriptions",
        )
        .with_long(
            "Fetch an already-registered application's remote config and its latest \
            release's webhook subscriptions, and write them to a godaddy.toml manifest \
            in the current directory. Syncs identity, version, and webhook subscriptions \
            from the remote application, while preserving locally-authored sections \
            (actions, dependencies, extensions, settings). Read-only against the API (no \
            application is created, no .env is written), so it's safe to re-run to \
            re-sync webhook subscriptions after a new release. Use `gddy platform app \
            update` to change label/description; url, proxy-url, and scopes are not \
            currently editable after registration.",
        )
        .with_system("applications")
        .with_tier(Tier::Mutate)
        .with_scopes(&[APP_REGISTRY_READ])
        .with_output_schema::<ApplicationImport>()
        .with_view(import_view_columns()),
        |ctx, args: ImportArgs| async move {
            run(&ctx, args.name, args.force, ImportOverrides::default()).await
        },
    )
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        existing_config_for, import_view_columns, latest_release, read_existing_config,
        subscriptions_at_risk, subscriptions_from_latest_release,
    };
    use crate::config::{Config, SubscriptionConfig};

    fn import_clap_command() -> clap::Command {
        super::command().spec.clap_command()
    }

    fn sub(name: &str, url: &str, events: &[&str]) -> SubscriptionConfig {
        SubscriptionConfig {
            name: name.to_owned(),
            url: url.to_owned(),
            events: events.iter().map(|e| (*e).to_owned()).collect(),
        }
    }

    fn config_for(name: &str) -> Config {
        Config {
            name: name.to_owned(),
            client_id: "client-1".to_owned(),
            description: None,
            version: "1.0.0".to_owned(),
            url: "https://example.com".to_owned(),
            proxy_url: "https://proxy.example.com".to_owned(),
            authorization_scopes: vec![],
            actions: vec![],
            subscriptions: None,
            dependencies: vec![],
            extensions: None,
            settings: vec![],
        }
    }

    #[test]
    fn import_requires_a_name() {
        let err = import_clap_command()
            .try_get_matches_from(["import"])
            .expect_err("import should require a NAME positional");
        assert_eq!(err.kind(), clap::error::ErrorKind::MissingRequiredArgument);
    }

    #[test]
    fn import_accepts_name_and_force() {
        import_clap_command()
            .try_get_matches_from(["import", "my-app", "--force"])
            .expect("import <name> --force should parse");
    }

    #[test]
    fn subscriptions_at_risk_is_empty_when_lists_match_ignoring_order() {
        let local = vec![
            sub("a", "/a", &["evt.a", "evt.b"]),
            sub("b", "/b", &["evt.c"]),
        ];
        // Same content, different subscription order and different event order.
        let remote = vec![
            sub("b", "/b", &["evt.c"]),
            sub("a", "/a", &["evt.b", "evt.a"]),
        ];
        assert!(subscriptions_at_risk(&local, &remote).is_empty());
    }

    #[test]
    fn subscriptions_at_risk_flags_local_only_entries() {
        let local = vec![
            sub("a", "/a", &["evt.a"]),
            sub("unpublished", "/u", &["evt.z"]),
        ];
        let remote = vec![sub("a", "/a", &["evt.a"])];
        assert_eq!(subscriptions_at_risk(&local, &remote), vec!["unpublished"]);
    }

    #[test]
    fn subscriptions_at_risk_flags_a_modified_entry() {
        let local = vec![sub("a", "/a-new", &["evt.a"])];
        let remote = vec![sub("a", "/a-old", &["evt.a"])];
        assert_eq!(subscriptions_at_risk(&local, &remote), vec!["a"]);
    }

    #[test]
    fn subscriptions_from_latest_release_relativizes_urls() {
        let app = json!({
            "releases": {
                "edges": [{
                    "node": {
                        "subscriptions": [{
                            "name": "order-notifications",
                            "url": "https://proxy.example.com/webhooks/orders",
                            "events": ["commerce.order.created", "commerce.order.updated"],
                        }]
                    }
                }]
            }
        });
        let subs = subscriptions_from_latest_release(&app, "https://proxy.example.com");
        assert_eq!(subs.len(), 1);
        assert_eq!(subs[0].name, "order-notifications");
        assert_eq!(subs[0].url, "/webhooks/orders");
        assert_eq!(
            subs[0].events,
            vec!["commerce.order.created", "commerce.order.updated"]
        );
    }

    #[test]
    fn subscriptions_from_latest_release_is_empty_without_releases() {
        let app = json!({ "releases": { "edges": [] } });
        assert!(subscriptions_from_latest_release(&app, "https://proxy.example.com").is_empty());
    }

    #[test]
    fn latest_release_exposes_the_release_version() {
        let app = json!({
            "releases": { "edges": [{ "node": { "version": "1.4.2" } }] }
        });
        assert_eq!(
            latest_release(&app).and_then(|node| node["version"].as_str()),
            Some("1.4.2")
        );
    }

    #[test]
    fn latest_release_is_none_without_releases() {
        let app = json!({ "releases": { "edges": [] } });
        assert!(latest_release(&app).is_none());
    }

    /// Proves `import_view_columns()` renders a `filesWritten` shaped like what
    /// the `import` handler actually builds (`config` path, confirmed by
    /// inspection) as a nested property bag — a column/field name mismatch
    /// would silently drop the write summary from human output and print a
    /// raw JSON blob instead. Renders a hand-built envelope rather than
    /// calling the handler, which would need a live app-registry API call.
    #[test]
    fn import_result_renders_files_written_as_a_nested_property_bag() {
        let result = json!({
            "id": "app-1",
            "name": "demo",
            "status": "ACTIVE",
            "filesWritten": {
                "config": "/home/user/project/godaddy.toml",
            },
        });
        let envelope = cli_engine::Envelope::success(result, "applications");
        let rendered =
            cli_engine::render_human_with_view(&envelope, Some(&import_view_columns()), "");
        assert!(rendered.contains("Files Written:"), "{rendered}");
        assert!(rendered.contains("godaddy.toml"), "{rendered}");
    }

    #[test]
    fn read_existing_config_treats_a_missing_file_as_none() {
        let dir =
            std::env::temp_dir().join(format!("gddy-import-test-missing-{}", std::process::id()));
        let path = dir.join("godaddy.toml");
        assert!(
            read_existing_config(&path)
                .expect("missing file is not an error")
                .is_none()
        );
    }

    #[test]
    fn read_existing_config_propagates_parse_errors_instead_of_swallowing_them() {
        let dir =
            std::env::temp_dir().join(format!("gddy-import-test-corrupt-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let path = dir.join("godaddy.toml");
        std::fs::write(&path, "this is not valid toml {{{").expect("write corrupt config");

        let err = read_existing_config(&path)
            .expect_err("a corrupt (but present) manifest must not be treated as \"no config\"");
        assert!(err.to_string().contains("failed to read existing config"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn existing_config_for_preserves_a_matching_manifest() {
        let existing = Some(config_for("demo"));
        let result = existing_config_for(existing, "demo", false)
            .expect("matching name should be preserved");
        assert_eq!(result.map(|c| c.name), Some("demo".to_owned()));
    }

    #[test]
    fn existing_config_for_rejects_a_mismatched_manifest_without_force() {
        let existing = Some(config_for("app-a"));
        let err = existing_config_for(existing, "app-b", false)
            .expect_err("importing a different app over an existing manifest must be rejected");
        assert!(err.to_string().contains("app-a"));
        assert!(err.to_string().contains("app-b"));
    }

    #[test]
    fn existing_config_for_discards_a_mismatched_manifest_with_force() {
        let existing = Some(config_for("app-a"));
        let result = existing_config_for(existing, "app-b", true)
            .expect("force should allow overwriting a different application's manifest");
        assert!(
            result.is_none(),
            "a different application's local sections must not be carried over, even with force"
        );
    }
}
