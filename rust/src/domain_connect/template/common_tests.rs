use httpmock::prelude::*;

use super::*;

fn template() -> Template {
    let value = json!({
        "providerId": "acme.com",
        "providerName": "Acme",
        "serviceId": "svc",
        "serviceName": "Svc",
        "version": 1,
        "warnPhishing": true,
        "futureField": { "a": 1 },
        "records": [{ "type": "A", "host": "@", "pointsTo": "203.0.113.10", "ttl": "%ttl%" }]
    });
    value.as_object().expect("object").clone()
}

fn client(server: &MockServer) -> domains_client::Client {
    domains_client::client_with_builder(
        reqwest::Client::builder(),
        &server.base_url(),
        "Bearer test-token",
        "req-1",
    )
    .expect("build client")
}

fn fix_of(err: &cli_engine::CliCoreError) -> String {
    cli_engine::build_error_envelope(err, SYSTEM)
        .fix
        .unwrap_or_default()
}

fn code_of(err: &cli_engine::CliCoreError) -> String {
    cli_engine::build_error_envelope(err, SYSTEM)
        .error
        .map(|error| error.code)
        .unwrap_or_default()
}

#[tokio::test]
async fn validate_sends_the_template_as_written_and_reads_the_outcome() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/v2/domains/validate-domain-connect-template")
                .header("authorization", "Bearer test-token")
                .header_exists("x-request-id")
                .json_body(json!({ "template": Value::Object(template()) }));
            then.status(200).json_body(json!({
                "action": "VALIDATE", "outcome": "READY", "operation": "ADD", "issues": []
            }));
        })
        .await;

    let result = validate(&client(&server), &template()).await.expect("ok");

    mock.assert_async().await;
    assert_eq!(result.outcome.as_str(), READY);
    assert!(result.issues.is_empty());
}

#[tokio::test]
async fn submit_returns_the_pull_request_when_submitted() {
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/v2/domains/submit-domain-connect-template")
                .json_body(json!({ "template": Value::Object(template()) }));
            then.status(200).json_body(json!({
                "action": "SUBMIT", "outcome": "SUBMITTED", "operation": "ADD", "issues": [],
                "pullRequest": { "number": 1234, "url": "https://github.com/Domain-Connect/Templates/pull/1234", "state": "OPEN" }
            }));
        })
        .await;

    let result = submit(&client(&server), &template()).await.expect("ok");

    assert_eq!(result.outcome.as_str(), SUBMITTED);
    let pull_request = result.pull_request.expect("pull request");
    assert_eq!(pull_request.number, Some(1234));
    assert_eq!(
        pull_request
            .state
            .as_ref()
            .map(ToString::to_string)
            .as_deref(),
        Some("OPEN")
    );
}

#[tokio::test]
async fn submit_reads_a_refusal_from_a_200() {
    let server = MockServer::start_async().await;
    for (outcome, code) in [
        ("BLOCKED", "PROVIDER_OWNED_BY_OTHER"),
        ("INVALID", "SCHEMA"),
    ] {
        let mock = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v2/domains/submit-domain-connect-template");
                then.status(200).json_body(json!({
                    "action": "SUBMIT", "outcome": outcome, "operation": "ADD",
                    "issues": [{ "code": code, "level": "ERROR", "message": "m", "field": "providerId" }]
                }));
            })
            .await;

        let result = submit(&client(&server), &template()).await.expect("result");

        assert_eq!(result.outcome.as_str(), outcome);
        assert_eq!(result.issues[0].code, code);
        assert!(result.pull_request.is_none());
        mock.delete_async().await;
    }
}

#[tokio::test]
async fn a_template_that_cannot_be_read_is_an_error_on_both_operations() {
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(POST);
            then.status(422).json_body(json!({
                "name": "UNPROCESSABLE_ENTITY",
                "message": "The template could not be processed.",
                "details": [{ "field": "/template/records", "issue": "INVALID_TYPE",
                              "description": "records must be an array." }]
            }));
        })
        .await;
    let client = client(&server);

    let errors = [
        validate(&client, &template()).await.expect_err("validate"),
        submit(&client, &template()).await.expect_err("submit"),
    ];

    for err in errors {
        let text = err.to_string();
        assert!(text.contains("UNPROCESSABLE_ENTITY"), "{text}");
        assert!(text.contains("records must be an array."), "{text}");
        assert!(fix_of(&err).contains("records list"), "{}", fix_of(&err));
    }
}

#[tokio::test]
async fn submit_does_not_read_a_result_from_an_error_status() {
    let server = MockServer::start_async().await;
    for status in [409, 422] {
        let mock = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v2/domains/submit-domain-connect-template");
                then.status(status).json_body(json!({
                    "action": "SUBMIT", "outcome": "SUBMITTED", "issues": []
                }));
            })
            .await;

        let err = submit(&client(&server), &template())
            .await
            .expect_err("an error status cannot be a success");

        assert!(err.to_string().contains(&status.to_string()), "{err}");
        mock.delete_async().await;
    }
}

#[tokio::test]
async fn an_outcome_or_state_added_later_is_read_as_text() {
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/v2/domains/submit-domain-connect-template");
            then.status(200).json_body(json!({
                "action": "SUBMIT", "outcome": "QUEUED", "operation": "REPLACE", "issues": [],
                "pullRequest": { "number": 7, "url": "https://example.test/7", "state": "DRAFT" }
            }));
        })
        .await;

    let result = submit(&client(&server), &template()).await.expect("ok");
    let data = result_json(Path::new("t.json"), &template(), &result);

    assert_eq!(data["outcome"], "QUEUED");
    assert_eq!(data["operation"], "REPLACE");
    assert_eq!(data["pullRequest"]["state"], "DRAFT");
}

#[tokio::test]
async fn validate_treats_409_as_an_error() {
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/v2/domains/validate-domain-connect-template");
            then.status(409).json_body(json!({
                "action": "VALIDATE", "outcome": "BLOCKED", "issues": []
            }));
        })
        .await;

    let err = validate(&client(&server), &template())
        .await
        .expect_err("409 is not a validate status");

    assert!(err.to_string().contains("409"), "{err}");
}

#[tokio::test]
async fn a_success_missing_its_outcome_is_an_error_not_an_empty_result() {
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/v2/domains/validate-domain-connect-template");
            then.status(200)
                .json_body(json!({ "name": "INTERNAL", "message": "boom" }));
        })
        .await;

    let err = validate(&client(&server), &template())
        .await
        .expect_err("no outcome");

    assert_eq!(code_of(&err), "UNEXPECTED_ERROR");
}

#[tokio::test]
async fn statuses_keep_their_own_error_codes_and_fixes() {
    let server = MockServer::start_async().await;
    for (status, code, fix) in [
        (401, "AUTH_REQUIRED", "gddy auth login"),
        (403, "NETWORK_ERROR", "not allowed to check"),
        (404, "NOT_FOUND", "Domain Connect template service"),
        (429, "NETWORK_ERROR", "Wait a minute"),
        (503, "NETWORK_ERROR", "server-side"),
    ] {
        let mock = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v2/domains/validate-domain-connect-template");
                then.status(status).json_body(json!({
                    "name": "FAILED", "correlationId": "550e8400-e29b-41d4-a716-446655440000",
                    "message": "Something went wrong."
                }));
            })
            .await;

        let err = validate(&client(&server), &template())
            .await
            .expect_err("error status");

        assert_eq!(code_of(&err), code, "status {status}");
        assert!(
            fix_of(&err).contains(fix),
            "status {status}: {}",
            fix_of(&err)
        );
        assert!(
            err.to_string().contains("FAILED: Something went wrong."),
            "{err}"
        );
        mock.delete_async().await;
    }
}

#[test]
fn a_403_fix_names_no_internal_permission() {
    for submitting in [false, true] {
        let fix = fix_of(&http_error(403, "{}", submitting));
        assert!(fix.contains("gddy auth login"), "{fix}");
        assert!(!fix.contains("domain-connect-template"), "{fix}");
        assert!(!fix.contains("scope"), "{fix}");
    }
}

#[test]
fn read_template_keeps_unmodeled_fields_and_string_numbers() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("acme.com.svc.json");
    std::fs::write(
        &path,
        serde_json::to_string(&Value::Object(template())).expect("encode"),
    )
    .expect("write");

    let (shown, read) = read_template(path.to_str().expect("utf8")).expect("reads");

    assert_eq!(shown, path);
    assert_eq!(read, template());
    assert_eq!(read["records"][0]["ttl"], "%ttl%");
}

#[test]
fn read_template_rejects_what_is_not_a_template_object() {
    let dir = tempfile::tempdir().expect("tempdir");
    let list = dir.path().join("list.json");
    std::fs::write(&list, "[1,2,3]").expect("write");
    let broken = dir.path().join("broken.json");
    std::fs::write(&broken, "{").expect("write");

    let err = read_template(list.to_str().expect("utf8")).expect_err("not an object");
    assert!(err.to_string().contains("JSON object"), "{err}");
    assert_eq!(code_of(&err), "VALIDATION_ERROR");

    let err = read_template(broken.to_str().expect("utf8")).expect_err("not JSON");
    assert!(err.to_string().contains("not valid JSON"), "{err}");
    assert!(fix_of(&err).contains("template init"), "{}", fix_of(&err));
}

#[test]
fn read_template_reports_a_missing_file_and_a_directory_differently() {
    let missing = read_template("/definitely/not/here.json").expect_err("missing");
    assert_eq!(code_of(&missing), "NOT_FOUND");
    assert!(
        fix_of(&missing).contains("template init"),
        "{}",
        fix_of(&missing)
    );

    let dir = tempfile::tempdir().expect("tempdir");
    let err = read_template(dir.path().to_str().expect("utf8")).expect_err("a directory");
    assert_eq!(code_of(&err), "VALIDATION_ERROR");
    assert!(err.to_string().contains("Could not read"), "{err}");
}

fn result(value: Value) -> TemplateAssessment {
    serde_json::from_value(value).expect("result fixture")
}

#[test]
fn result_json_reports_issues_and_the_pull_request_only_when_present() {
    let invalid = result(json!({
        "action": "VALIDATE", "outcome": "INVALID", "operation": "ADD",
        "issues": [{ "code": "DCTL1012", "level": "ERROR", "message": "syncPubKeyDomain is required",
                     "field": "syncPubKeyDomain", "fix": "Add syncPubKeyDomain" }]
    }));

    let data = result_json(Path::new("acme.com.svc.json"), &template(), &invalid);

    assert_eq!(data["providerId"], "acme.com");
    assert_eq!(data["outcome"], "INVALID");
    assert_eq!(data["operation"], "ADD");
    assert_eq!(
        data["issues"][0],
        json!({ "level": "ERROR", "code": "DCTL1012", "field": "syncPubKeyDomain",
                "message": "syncPubKeyDomain is required", "fix": "Add syncPubKeyDomain" })
    );
    assert!(data.get("pullRequest").is_none());
}

#[test]
fn the_validate_view_lists_each_issue_with_its_fix() {
    // Rendered at the width a pipe gets, where the table is narrowest.
    let invalid = result(json!({
        "action": "VALIDATE", "outcome": "INVALID", "operation": "ADD",
        "issues": [{ "code": "DCTL1012", "level": "ERROR",
                     "message": "syncPubKeyDomain is required when syncRedirectDomain is set",
                     "field": "syncPubKeyDomain",
                     "fix": "Add syncPubKeyDomain or remove syncRedirectDomain" }]
    }));
    let data = result_json(Path::new("acme.com.svc.json"), &template(), &invalid);

    let rendered = cli_engine::preview_human_view(&data, &validate_columns());

    assert!(rendered.contains("Outcome: INVALID"), "{rendered}");
    for cell in [
        "ERROR",
        "syncPubKeyDomain is required when syncRedirectDomain is set",
        "Add syncPubKeyDomain or remove syncRedirectDomain",
    ] {
        assert!(rendered.contains(cell), "missing {cell:?} in:\n{rendered}");
    }
}

#[test]
fn a_ready_template_renders_with_no_issue_rows() {
    let ready = result(json!({
        "action": "VALIDATE", "outcome": "READY", "operation": "ADD", "issues": []
    }));
    let data = result_json(Path::new("acme.com.svc.json"), &template(), &ready);

    let rendered = cli_engine::preview_human_view(&data, &validate_columns());

    assert!(rendered.contains("Outcome: READY"), "{rendered}");
    assert!(!rendered.contains("ERROR"), "{rendered}");
}

#[test]
fn the_submit_view_shows_the_pull_request_link_and_state() {
    let submitted = result(json!({
        "action": "SUBMIT", "outcome": "SUBMITTED", "operation": "ADD", "issues": [],
        "pullRequest": { "number": 1234, "url": "https://github.com/Domain-Connect/Templates/pull/1234", "state": "OPEN" }
    }));
    let data = result_json(Path::new("acme.com.svc.json"), &template(), &submitted);

    let rendered = cli_engine::preview_human_view(&data, &submit_columns());

    assert!(rendered.contains("Outcome: SUBMITTED"), "{rendered}");
    assert!(
        rendered.contains("https://github.com/Domain-Connect/Templates/pull/1234"),
        "{rendered}"
    );
    assert!(rendered.contains("OPEN"), "{rendered}");
}

#[test]
fn every_view_column_is_a_field_of_the_result() {
    let submitted = result(json!({
        "action": "SUBMIT", "outcome": "SUBMITTED", "operation": "ADD", "issues": [],
        "pullRequest": { "number": 1, "url": "https://example.test/1", "state": "OPEN" }
    }));
    let data = result_json(Path::new("t.json"), &template(), &submitted);

    for column in validate_columns().iter().chain(&submit_columns()) {
        assert!(
            data.get(&column.field).is_some(),
            "{} is not in the result",
            column.field
        );
    }
}
