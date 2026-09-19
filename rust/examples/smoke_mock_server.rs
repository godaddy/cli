//! Mock `app-registry-api` for `rust/scripts/smoke-test.sh`. Dev-only —
//! must stay a Cargo example, never a `[[bin]]` (uses the `httpmock` dev-dependency).
#![allow(clippy::print_stdout)]

use std::thread;
use std::time::Duration;

use httpmock::{HttpMockRequest, HttpMockResponse, MockServer};
use serde_json::{Value, json};

const GRAPHQL_PATH: &str = "/v1/apps/app-registry-subgraph";

/// `applicationId` values that select a canned error response instead of
/// the normal success echo, so the smoke test can drive real HTTP/GraphQL
/// error handling without a second mock process.
const GRAPHQL_ERROR_APPLICATION_ID: &str = "smoke-graphql-error-id";
const HTTP_500_APPLICATION_ID: &str = "smoke-http-500-id";
const HTTP_401_APPLICATION_ID: &str = "smoke-http-401-id";

fn create_release_response(req: &HttpMockRequest) -> HttpMockResponse {
    let body: Value = match serde_json::from_slice(&req.body_bytes()) {
        Ok(v) => v,
        Err(e) => {
            return HttpMockResponse::builder()
                .status(400)
                .body(format!("smoke mock: request body is not JSON: {e}"))
                .build();
        }
    };
    let query = body["query"].as_str().unwrap_or_default();
    if !query.contains("CreateRelease") {
        return HttpMockResponse::builder()
            .status(400)
            .body(format!(
                "smoke mock: only CreateRelease is mocked, got query: {query}"
            ))
            .build();
    }

    let input = &body["variables"]["input"];
    match input["applicationId"].as_str() {
        Some(HTTP_500_APPLICATION_ID) => {
            return HttpMockResponse::builder()
                .status(500)
                .body("smoke mock: internal server error")
                .build();
        }
        Some(HTTP_401_APPLICATION_ID) => {
            return HttpMockResponse::builder()
                .status(401)
                .body("smoke mock: unauthorized")
                .build();
        }
        Some(GRAPHQL_ERROR_APPLICATION_ID) => {
            return HttpMockResponse::builder()
                .status(200)
                .header("content-type", "application/json")
                .body(
                    json!({ "data": null, "errors": [{ "message": "release not found" }] })
                        .to_string(),
                )
                .build();
        }
        _ => {}
    }

    let settings: Vec<Value> = input["settings"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .enumerate()
        .map(|(i, mut setting)| {
            let obj = setting.as_object_mut().expect("setting is an object");
            obj.entry("id")
                .or_insert_with(|| json!(format!("smoke-setting-{i}")));
            obj.entry("capabilities").or_insert_with(|| json!([]));
            if obj.get("capabilities") == Some(&Value::Null) {
                obj.insert("capabilities".to_owned(), json!([]));
            }
            obj.entry("order").or_insert_with(|| json!(0));
            if obj.get("order") == Some(&Value::Null) {
                obj.insert("order".to_owned(), json!(0));
            }
            setting
        })
        .collect();
    let ui_extensions: Vec<Value> = input["uiExtensions"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .enumerate()
        .map(|(i, mut extension)| {
            let obj = extension.as_object_mut().expect("extension is an object");
            obj.entry("id")
                .or_insert_with(|| json!(format!("smoke-ui-extension-{i}")));
            obj.entry("type").or_insert_with(|| json!("embed"));
            extension
        })
        .collect();
    let release = json!({
        "id": "smoke-release-id",
        "version": input.get("version").cloned().unwrap_or(json!("0.0.0")),
        "description": input.get("description").cloned().unwrap_or(Value::Null),
        "createdAt": "2026-01-01T00:00:00Z",
        "uiExtensions": ui_extensions,
        "settings": settings,
    });
    HttpMockResponse::builder()
        .status(200)
        .header("content-type", "application/json")
        .body(json!({ "data": { "createRelease": release } }).to_string())
        .build()
}

fn main() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method("POST").path(GRAPHQL_PATH);
        then.respond_with(create_release_response);
    });
    println!("PORT={}", server.port());
    loop {
        thread::sleep(Duration::from_secs(3600));
    }
}
