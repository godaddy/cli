//! `submit`: which request key a create sends, and when the saved key is kept
//! or dropped, against a mock service and a key file in a temporary directory.

use std::path::PathBuf;

use httpmock::prelude::*;
use serde_json::json;

use super::*;

const NAME: &str = "my-site";
const ISSUE_BODY: &str = r#"{"message":"m","details":[{"issue":"ISSUE"}]}"#;

struct Setup {
    server: MockServer,
    _dir: tempfile::TempDir,
    keys: PathBuf,
}

async fn setup() -> Setup {
    let dir = tempfile::tempdir().expect("tempdir");
    let keys = dir.path().join("hosting-create-keys.json");
    Setup {
        server: MockServer::start_async().await,
        _dir: dir,
        keys,
    }
}

impl Setup {
    fn client(&self) -> HostingClient {
        HostingClient::new(self.server.base_url(), "test-token")
    }

    async fn submit(&self) -> cli_engine::Result<Value> {
        submit(&self.client(), Some(&self.keys), HostingAppType::Mhwp, NAME).await
    }

    /// Answers every create with `status` and `body`.
    async fn answer(&self, status: u16, body: &str) -> httpmock::Mock<'_> {
        let body = body.to_owned();
        self.server
            .mock_async(move |when, then| {
                when.method(POST)
                    .path("/v1/hosting/apps")
                    .query_param("appType", "MHWP")
                    .header_exists("idempotency-key");
                then.status(status)
                    .header("content-type", "application/json")
                    .body(body);
            })
            .await
    }

    /// Answers a create carrying `key` with an accepted operation.
    async fn accept(&self, key: &str) -> httpmock::Mock<'_> {
        let key = key.to_owned();
        self.server
            .mock_async(move |when, then| {
                when.method(POST)
                    .path("/v1/hosting/apps")
                    .header("idempotency-key", key);
                then.status(202).json_body(json!({
                    "operationId": "MHWP-op12345",
                    "type": "CREATE_APP",
                    "status": "PENDING",
                    "links": []
                }));
            })
            .await
    }

    /// The key a create of `NAME` would send now, without sending one.
    fn saved_key(&self) -> String {
        let base_url = self.server.base_url();
        let claim = create_keys::claim(
            Some(&self.keys),
            &base_url,
            HostingAppType::Mhwp,
            NAME,
            Utc::now(),
        );
        claim.key
    }
}

fn issue(issue: &str) -> String {
    ISSUE_BODY.replace("ISSUE", issue)
}

#[tokio::test]
async fn an_accepted_create_drops_its_key_so_the_next_run_is_a_new_create() {
    let s = setup().await;
    let key = s.saved_key();
    let first = s.accept(&key).await;

    let data = s.submit().await.expect("create accepted");

    first.assert_async().await;
    assert_eq!(data["operationId"], "MHWP-op12345");
    assert_ne!(
        s.saved_key(),
        key,
        "an accepted create must not be replayed"
    );
}

#[tokio::test]
async fn a_server_error_keeps_the_key_for_the_next_run() {
    let s = setup().await;
    let failing = s.answer(503, &issue("UPSTREAM_UNAVAILABLE")).await;

    let err = s.submit().await.expect_err("server error");
    failing.assert_async().await;
    let fix = cli_engine::build_error_envelope(&err, "hosting").fix;
    assert!(
        fix.as_deref().is_some_and(|f| f.contains("same --name")),
        "{fix:?}"
    );

    failing.delete_async().await;
    let key = s.saved_key();
    let replay = s.accept(&key).await;
    s.submit().await.expect("same create accepted");
    replay.assert_async().await;
}

#[tokio::test]
async fn a_lost_connection_keeps_the_key_for_the_next_run() {
    let dir = tempfile::tempdir().expect("tempdir");
    let keys = dir.path().join("hosting-create-keys.json");
    let closed = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let url = format!("http://{}", closed.local_addr().expect("addr"));
    drop(closed);
    let unreachable = HostingClient::new(url.clone(), "test-token");
    let saved = |now| create_keys::claim(Some(&keys), &url, HostingAppType::Mhwp, NAME, now).key;
    let before = saved(Utc::now());

    let err = submit(&unreachable, Some(&keys), HostingAppType::Mhwp, NAME)
        .await
        .expect_err("connection refused");

    assert_eq!(saved(Utc::now()), before);
    let fix = cli_engine::build_error_envelope(&err, "hosting").fix;
    assert!(
        fix.as_deref()
            .is_some_and(|f| f.contains("may have reached GoDaddy") && f.contains("same --name")),
        "{fix:?}"
    );
}

#[tokio::test]
async fn a_create_still_in_progress_keeps_its_key() {
    let s = setup().await;
    s.answer(409, &issue("IDEMPOTENCY_IN_PROGRESS")).await;
    let before = s.saved_key();

    s.submit().await.expect_err("in progress");

    assert_eq!(s.saved_key(), before);
}

#[tokio::test]
async fn a_refusal_keeps_its_key_and_its_fix() {
    let s = setup().await;
    s.answer(422, &issue("APP_LIMIT_EXCEEDED")).await;
    let before = s.saved_key();

    let err = s.submit().await.expect_err("refused");

    assert_eq!(s.saved_key(), before);
    let fix = cli_engine::build_error_envelope(&err, "hosting").fix;
    assert!(
        fix.as_deref().is_some_and(|f| f.contains("app limit")),
        "{fix:?}"
    );
}

#[tokio::test]
async fn a_key_that_no_longer_fits_is_dropped() {
    let s = setup().await;
    s.answer(422, &issue("IDEMPOTENCY_KEY_MISMATCH")).await;
    let before = s.saved_key();

    s.submit().await.expect_err("mismatch");

    assert_ne!(s.saved_key(), before);
}

#[tokio::test]
async fn an_accepted_answer_that_cannot_be_read_keeps_the_key() {
    let s = setup().await;
    // A cut-off body: the service may have accepted the create.
    s.answer(202, r#"{"operationId":"MHWP-op1"#).await;
    let before = s.saved_key();

    let err = s.submit().await.expect_err("unreadable answer");

    assert_eq!(s.saved_key(), before);
    let fix = cli_engine::build_error_envelope(&err, "hosting").fix;
    assert!(
        fix.as_deref()
            .is_some_and(|f| f.contains("may have reached GoDaddy") && f.contains("same --name")),
        "{fix:?}"
    );
}

#[tokio::test]
async fn an_accepted_answer_without_an_operation_keeps_the_key() {
    // An empty reply (from a proxy, say) or one without an operation ID does
    // not show whether the create started, so the next run must replay it.
    for body in ["", "{}", r#"{"operationId":""}"#, r#"{"status":"PENDING"}"#] {
        let s = setup().await;
        s.answer(202, body).await;
        let before = s.saved_key();

        let err = s.submit().await.expect_err("no operation ID");

        assert_eq!(s.saved_key(), before, "{body:?}");
        let fix = cli_engine::build_error_envelope(&err, "hosting").fix;
        assert!(
            fix.as_deref().is_some_and(
                |f| f.contains("may have reached GoDaddy") && f.contains("same --name")
            ),
            "{body:?}: {fix:?}"
        );
    }
}

#[tokio::test]
async fn a_replayed_failed_create_drops_its_key_so_the_next_run_is_a_new_create() {
    let s = setup().await;
    let key = s.saved_key();
    // After a lost answer the service replays the first operation, here one
    // that has ended FAILED: the key has done its job, and the next run must be
    // a real new create, as the service asks for a FAILED create.
    let replay = s
        .server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/v1/hosting/apps")
                .header("idempotency-key", key.clone());
            then.status(202).json_body(json!({
                "operationId": "MHWP-op12345",
                "type": "CREATE_APP",
                "status": "FAILED",
                "error": {
                    "name": "AppCreationFailed",
                    "message": "No capacity is available to create the app. Retry later.",
                    "details": [{ "issue": "CAPACITY_UNAVAILABLE" }]
                },
                "links": []
            }));
        })
        .await;

    let data = s.submit().await.expect("replayed operation");

    replay.assert_async().await;
    assert_eq!(data["status"], "FAILED");
    assert_ne!(s.saved_key(), key);
}

#[cfg(unix)]
#[tokio::test]
async fn a_key_that_cannot_be_removed_does_not_fail_an_accepted_create() {
    use std::os::unix::fs::PermissionsExt as _;

    let s = setup().await;
    let key = s.saved_key();
    let accepted = s.accept(&key).await;
    // A read-only directory: the saved key still reads, but removing it after
    // the 202 cannot write. That must not fail a create the service accepted.
    let dir = s._dir.path();
    let set_mode =
        |mode| std::fs::set_permissions(dir, std::fs::Permissions::from_mode(mode)).expect("chmod");
    set_mode(0o555);
    if std::fs::write(dir.join("probe"), "").is_ok() {
        // Root ignores directory permissions, so the failure cannot be made.
        set_mode(0o755);
        return;
    }

    let result = s.submit().await;
    set_mode(0o755);

    accepted.assert_async().await;
    assert_eq!(
        result.expect("create accepted")["operationId"],
        "MHWP-op12345"
    );
    assert_eq!(
        s.saved_key(),
        key,
        "the remove failed, so the key is still there"
    );
}

#[tokio::test]
async fn every_server_error_keeps_the_key() {
    for status in [500, 502, 503, 504] {
        let s = setup().await;
        s.answer(status, &issue("UPSTREAM_UNAVAILABLE")).await;
        let before = s.saved_key();

        let err = s.submit().await.expect_err("server error");

        assert_eq!(s.saved_key(), before, "{status}");
        let fix = cli_engine::build_error_envelope(&err, "hosting").fix;
        assert!(
            fix.as_deref().is_some_and(|f| f.contains("same --name")),
            "{status}: {fix:?}"
        );
    }
}

#[tokio::test]
async fn a_create_without_a_saved_key_still_sends_one_and_promises_nothing() {
    let s = setup().await;
    s.answer(503, &issue("UPSTREAM_UNAVAILABLE")).await;

    let err = submit(&s.client(), None, HostingAppType::Mhwp, NAME)
        .await
        .expect_err("server error");

    let fix = cli_engine::build_error_envelope(&err, "hosting").fix;
    assert!(
        fix.as_deref()
            .is_some_and(|f| f.contains("gddy hosting app list --app-type MHWP")
                && !f.contains("same --name")),
        "{fix:?}"
    );
}

#[tokio::test]
async fn a_key_about_to_expire_is_still_sent_but_promises_no_rerun() {
    let s = setup().await;
    // Saved almost a day ago: the next run sends this key, but it expires
    // before there is time to read the error and run the command again.
    let created_at = Utc::now() - chrono::TimeDelta::hours(24) + chrono::TimeDelta::minutes(5);
    let key = "11111111-1111-4111-8111-111111111111";
    let file = json!({ "entries": [{
        "baseUrl": s.server.base_url(),
        "appType": "MHWP",
        "name": NAME,
        "key": key,
        "createdAt": created_at,
    }]});
    std::fs::write(&s.keys, file.to_string()).expect("write key file");
    let failing = s
        .server
        .mock_async(move |when, then| {
            when.method(POST)
                .path("/v1/hosting/apps")
                .header("idempotency-key", key);
            then.status(503)
                .header("content-type", "application/json")
                .body(issue("UPSTREAM_UNAVAILABLE"));
        })
        .await;

    let err = s.submit().await.expect_err("server error");

    failing.assert_async().await;
    let fix = cli_engine::build_error_envelope(&err, "hosting").fix;
    assert!(
        fix.as_deref()
            .is_some_and(|f| f.contains("gddy hosting app list --app-type MHWP")
                && !f.contains("same --name")),
        "{fix:?}"
    );
}

#[tokio::test]
async fn a_fresh_key_names_the_time_a_rerun_is_safe_until() {
    let s = setup().await;
    s.answer(503, &issue("UPSTREAM_UNAVAILABLE")).await;
    // Saved first with a known time, so that the deadline the fix names and
    // the one expected here come from the same moment, even across a minute.
    let started = Utc::now();
    let base_url = s.server.base_url();
    create_keys::claim(
        Some(&s.keys),
        &base_url,
        HostingAppType::Mhwp,
        NAME,
        started,
    );

    let err = s.submit().await.expect_err("server error");

    let until = (started + chrono::TimeDelta::hours(24))
        .format("%Y-%m-%d %H:%M UTC")
        .to_string();
    let fix = cli_engine::build_error_envelope(&err, "hosting").fix;
    assert!(
        fix.as_deref()
            .is_some_and(|f| f.contains(&format!("same --name before {until}"))),
        "{until}: {fix:?}"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn a_mismatched_key_that_cannot_be_removed_says_how_to_clear_it() {
    use std::os::unix::fs::PermissionsExt as _;

    let s = setup().await;
    s.answer(422, &issue("IDEMPOTENCY_KEY_MISMATCH")).await;
    let key = s.saved_key();
    // A read-only directory: the key still reads, but it cannot be removed, so
    // a plain "run the command again" would send it, and be refused, again.
    let dir = s._dir.path();
    let set_mode =
        |mode| std::fs::set_permissions(dir, std::fs::Permissions::from_mode(mode)).expect("chmod");
    set_mode(0o555);
    if std::fs::write(dir.join("probe"), "").is_ok() {
        // Root ignores directory permissions, so the failure cannot be made.
        set_mode(0o755);
        return;
    }

    let err = s.submit().await.expect_err("mismatch");
    set_mode(0o755);

    assert_eq!(
        s.saved_key(),
        key,
        "the remove failed, so the key is still there"
    );
    let fix = cli_engine::build_error_envelope(&err, "hosting").fix;
    let file = s.keys.display().to_string();
    assert!(
        fix.as_deref()
            .is_some_and(|f| f.contains(&file) && f.contains("Make sure you can write")),
        "{fix:?}"
    );
}
