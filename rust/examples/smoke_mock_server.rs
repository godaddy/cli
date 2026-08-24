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
    let release = json!({
        "id": "smoke-release-id",
        "version": input.get("version").cloned().unwrap_or(json!("0.0.0")),
        "description": input.get("description").cloned().unwrap_or(Value::Null),
        "createdAt": "2026-01-01T00:00:00Z",
        "uiExtensions": input.get("uiExtensions").cloned().unwrap_or(json!([])),
        "settings": input.get("settings").cloned().unwrap_or(json!([])),
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
