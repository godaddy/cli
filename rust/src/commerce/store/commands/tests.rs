use std::sync::Arc;

use super::*;
use crate::{
    all_modules,
    commerce::module,
    environments::test_support::{ENV_LOCK, EnvGuard},
};
use cli_engine::{
    Cli, CliConfig, Stage,
    environments::{EnvTable, Environments},
    preview_human_view,
};
use commerce_client::client_with_builder;
use httpmock::prelude::*;
use reqwest::Client as HttpClient;
use serde_json::from_str;

const STORE_ID: &str = "6d1f0f4c-8a2e-4a52-9a5b-0f6a3c1d2e4b";

fn client_for(server: &MockServer) -> Client {
    client_with_builder(
        HttpClient::builder(),
        &server.base_url(),
        "Bearer tok",
        "req-1",
    )
    .expect("build client")
}

async fn serve_store(server: &MockServer) {
    server
        .mock_async(|when, then| {
            when.method(GET)
                .path(format!("/v1/commerce/stores/{STORE_ID}"));
            then.status(200).json_body(serde_json::json!({
                "id": STORE_ID,
                "businessId": "biz-1",
                "displayName": "Corner Shop",
                "status": "ACTIVE",
                "emailAddress": "shop@example.test",
                "currency": "USD"
            }));
        })
        .await;
}

async fn serve_status(server: &MockServer, status: u16) {
    server
        .mock_async(|when, then| {
            when.method(GET);
            then.status(status);
        })
        .await;
}

fn data(result: &CommandResult) -> &Value {
    &result.data
}

/// Runs `args` through the commerce command tree at the beta stage. The
/// saved selection lives wherever `XDG_CONFIG_HOME` points (see
/// [`isolated_config`]).
async fn run(args: &[&str]) -> (i32, Value) {
    let environments = Arc::new(Environments::new("ote").with_environment("ote", EnvTable::new()));
    let cli = Cli::new(
        CliConfig::new("gddy", "GoDaddy developer CLI", "gddy")
            .with_min_stage(Stage::Beta)
            .with_environments(environments)
            .with_startup_args(Vec::<&str>::new())
            .with_module(module()),
    );
    let output = cli.run(args.iter().copied()).await;
    let parsed = from_str(&output.rendered);
    assert!(parsed.is_ok(), "not json: {}", output.rendered);
    let json = parsed.expect("checked by the assertion above");
    (output.exit_code, json)
}

struct IsolatedConfig {
    _dir: tempfile::TempDir,
    _xdg: EnvGuard,
    _bare: EnvGuard,
    _per_env: EnvGuard,
}

fn isolated_config() -> IsolatedConfig {
    let dir = tempfile::tempdir().expect("temp dir");
    let xdg = EnvGuard::set("XDG_CONFIG_HOME", &dir.path().display().to_string());
    IsolatedConfig {
        _dir: dir,
        _xdg: xdg,
        _bare: EnvGuard::unset("GDDY_STORE_ID"),
        _per_env: EnvGuard::unset("GDDY_STORE_ID_OTE"),
    }
}

#[tokio::test]
async fn get_shows_the_store_in_camel_case() {
    let server = MockServer::start_async().await;
    serve_store(&server).await;

    let result = show(&client_for(&server), STORE_ID, "ote")
        .await
        .expect("the store is shown");

    let data = data(&result);
    assert_eq!(data["storeId"], STORE_ID);
    assert_eq!(data["displayName"], "Corner Shop");
    assert_eq!(data["status"], "ACTIVE");
    assert_eq!(data["businessId"], "biz-1");
    assert_eq!(data["emailAddress"], "shop@example.test");
    assert_eq!(data["currency"], "USD");
}

/// The human view is what a terminal user sees, so check the rendered text, not
/// only the data it is built from.
#[tokio::test]
async fn get_renders_a_readable_human_view() {
    let server = MockServer::start_async().await;
    serve_store(&server).await;
    let result = show(&client_for(&server), STORE_ID, "ote")
        .await
        .expect("the store is shown");

    let rendered = preview_human_view(result.data.clone(), &store_columns());

    for expected in ["Corner Shop", "ACTIVE", STORE_ID, "shop@example.test"] {
        assert!(
            rendered.contains(expected),
            "{expected} missing: {rendered}"
        );
    }
}

#[tokio::test]
async fn use_saves_the_store_it_fetched() {
    let _lock = ENV_LOCK.lock().await;
    let _config = isolated_config();
    let server = MockServer::start_async().await;
    serve_store(&server).await;

    let result = select(&client_for(&server), STORE_ID, "ote", false)
        .await
        .expect("the store is selected");

    assert_eq!(data(&result)["action"], "selected");
    let saved = context::saved_store("ote")
        .expect("the file reads")
        .expect("a selection was saved");
    assert_eq!(saved.store_id, STORE_ID);
    assert_eq!(saved.business_id.as_deref(), Some("biz-1"));
    assert_eq!(saved.display_name.as_deref(), Some("Corner Shop"));
}

/// A store that cannot be read must not become the selection, or every later
/// command would fail against an id that never worked.
#[tokio::test]
async fn use_saves_nothing_when_the_store_cannot_be_read() {
    let _lock = ENV_LOCK.lock().await;
    let _config = isolated_config();
    for status in [403, 404, 503] {
        let server = MockServer::start_async().await;
        serve_status(&server, status).await;

        select(&client_for(&server), STORE_ID, "ote", false)
            .await
            .expect_err("an unreadable store is an error");

        assert!(
            context::saved_store("ote")
                .expect("the file reads")
                .is_none(),
            "a {status} must not save a selection"
        );
    }
}

#[tokio::test]
async fn use_dry_run_reads_the_store_but_saves_nothing() {
    let _lock = ENV_LOCK.lock().await;
    let _config = isolated_config();
    let server = MockServer::start_async().await;
    serve_store(&server).await;

    let result = select(&client_for(&server), STORE_ID, "ote", true)
        .await
        .expect("the preview succeeds");

    assert_eq!(data(&result)["action"], "would select");
    assert!(result.metadata.dry_run, "a preview must be marked as one");
    assert!(
        context::saved_store("ote")
            .expect("the file reads")
            .is_none()
    );
}

#[tokio::test]
async fn use_dry_run_fails_where_the_real_run_would() {
    let _lock = ENV_LOCK.lock().await;
    let _config = isolated_config();
    let server = MockServer::start_async().await;
    serve_status(&server, 404).await;

    select(&client_for(&server), STORE_ID, "ote", true)
        .await
        .expect_err("a preview of a missing store is an error");
}

#[tokio::test]
async fn use_replaces_the_selection_for_that_environment_only() {
    let _lock = ENV_LOCK.lock().await;
    let _config = isolated_config();
    let server = MockServer::start_async().await;
    serve_store(&server).await;
    context::save_store(
        "prod",
        StoreEntry {
            store_id: "11111111-1111-4111-8111-111111111111".to_owned(),
            business_id: None,
            display_name: None,
        },
    )
    .expect("seed prod");

    select(&client_for(&server), STORE_ID, "ote", false)
        .await
        .expect("selected");

    let prod = context::saved_store("prod")
        .expect("the file reads")
        .expect("prod is kept");
    assert_eq!(prod.store_id, "11111111-1111-4111-8111-111111111111");
}

#[tokio::test]
async fn current_reports_no_selection_without_failing() {
    let _lock = ENV_LOCK.lock().await;
    let _config = isolated_config();

    let (exit, json) = run(&[
        "gddy", "commerce", "store", "current", "--env", "ote", "--output", "json",
    ])
    .await;

    assert_eq!(exit, 0, "{json}");
    assert_eq!(json["data"]["selected"], false);
    assert_eq!(json["data"]["environment"], "ote");
}

#[tokio::test]
async fn current_reports_the_saved_store_and_where_it_came_from() {
    let _lock = ENV_LOCK.lock().await;
    let _config = isolated_config();
    context::save_store(
        "ote",
        StoreEntry {
            store_id: STORE_ID.to_owned(),
            business_id: Some("biz-1".to_owned()),
            display_name: Some("Corner Shop".to_owned()),
        },
    )
    .expect("seed");

    let (exit, json) = run(&[
        "gddy", "commerce", "store", "current", "--env", "ote", "--output", "json",
    ])
    .await;

    assert_eq!(exit, 0, "{json}");
    assert_eq!(json["data"]["selected"], true);
    assert_eq!(json["data"]["storeId"], STORE_ID);
    assert_eq!(json["data"]["displayName"], "Corner Shop");
    assert_eq!(json["data"]["source"], "file");
}

/// An environment variable overrides the saved store, so `current` must say so
/// rather than claim the saved one is in effect.
#[tokio::test]
async fn current_says_when_an_environment_variable_overrides_the_saved_store() {
    let _lock = ENV_LOCK.lock().await;
    let _config = isolated_config();
    let _override = EnvGuard::set("GDDY_STORE_ID_OTE", "22222222-2222-4222-8222-222222222222");
    context::save_store(
        "ote",
        StoreEntry {
            store_id: STORE_ID.to_owned(),
            business_id: None,
            display_name: None,
        },
    )
    .expect("seed");

    let (_, json) = run(&[
        "gddy", "commerce", "store", "current", "--env", "ote", "--output", "json",
    ])
    .await;

    assert_eq!(
        json["data"]["storeId"],
        "22222222-2222-4222-8222-222222222222"
    );
    assert_eq!(json["data"]["source"], "env-per-env");
}

#[tokio::test]
async fn clear_removes_the_saved_store() {
    let _lock = ENV_LOCK.lock().await;
    let _config = isolated_config();
    context::save_store(
        "ote",
        StoreEntry {
            store_id: STORE_ID.to_owned(),
            business_id: None,
            display_name: None,
        },
    )
    .expect("seed");

    let (exit, json) = run(&[
        "gddy", "commerce", "store", "clear", "--env", "ote", "--output", "json",
    ])
    .await;

    assert_eq!(exit, 0, "{json}");
    assert_eq!(json["data"]["action"], "cleared");
    assert_eq!(json["data"]["storeId"], STORE_ID);
    assert!(context::saved_store("ote").expect("reads").is_none());
}

#[tokio::test]
async fn clear_with_nothing_saved_says_so_and_succeeds() {
    let _lock = ENV_LOCK.lock().await;
    let _config = isolated_config();

    let (exit, json) = run(&[
        "gddy", "commerce", "store", "clear", "--env", "ote", "--output", "json",
    ])
    .await;

    assert_eq!(exit, 0, "{json}");
    assert_eq!(json["data"]["action"], "nothing to clear");
}

#[tokio::test]
async fn clear_dry_run_keeps_the_saved_store() {
    let _lock = ENV_LOCK.lock().await;
    let _config = isolated_config();
    context::save_store(
        "ote",
        StoreEntry {
            store_id: STORE_ID.to_owned(),
            business_id: None,
            display_name: None,
        },
    )
    .expect("seed");

    let (exit, json) = run(&[
        "gddy",
        "commerce",
        "store",
        "clear",
        "--env",
        "ote",
        "--dry-run",
        "--output",
        "json",
    ])
    .await;

    assert_eq!(exit, 0, "{json}");
    assert_eq!(json["data"]["action"], "would clear");
    assert!(
        context::saved_store("ote").expect("reads").is_some(),
        "a preview must not remove the selection"
    );
}

#[test]
fn only_the_commands_that_call_the_api_need_the_store_scope() {
    for (command, reads_api) in [
        (get_command(), true),
        (use_command(), true),
        (current_command(), false),
        (clear_command(), false),
    ] {
        let spec = command.spec;
        let scopes = spec.metadata().scopes;
        assert_eq!(
            scopes == vec![COMMERCE_STORE_READ.to_owned()],
            reads_api,
            "{}: {scopes:?}",
            spec.name
        );
    }
}

#[test]
fn commands_that_change_state_are_marked_mutating_and_the_rest_read() {
    let tier = |command: RuntimeCommandSpec| command.spec.tier;
    assert_eq!(tier(get_command()), Some(Tier::Read));
    assert_eq!(tier(current_command()), Some(Tier::Read));
    assert_eq!(tier(use_command()), Some(Tier::Mutate));
    assert_eq!(tier(clear_command()), Some(Tier::Mutate));
}

/// The commerce group is in beta, so it must not appear at the default release
/// stage, and must once beta features are revealed.
#[tokio::test]
async fn the_commerce_group_is_hidden_at_ga_and_visible_from_beta() {
    let at = |stage| {
        Cli::new(
            CliConfig::new("gddy", "GoDaddy developer CLI", "gddy")
                .with_min_stage(stage)
                .with_modules(all_modules()),
        )
    };

    let hidden = at(Stage::Ga)
        .run(["gddy", "commerce", "store", "--help"])
        .await;
    assert_ne!(hidden.exit_code, 0, "{}", hidden.rendered);

    let shown = at(Stage::Beta)
        .run(["gddy", "commerce", "store", "--help"])
        .await;
    assert_eq!(shown.exit_code, 0, "{}", shown.rendered);
    for command in ["get", "use", "current", "clear"] {
        assert!(shown.rendered.contains(command), "{}", shown.rendered);
    }
}
