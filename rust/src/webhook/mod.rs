use cli_engine::{
    CommandResult, CommandSpec, GroupSpec, RuntimeCommandSpec, RuntimeGroupSpec, Tier,
};
use serde::{Deserialize, Serialize};

use crate::application::client::api_url_for_env;
use crate::error::GddyError;

/// One webhook event type returned by the API.
#[derive(Debug, Deserialize, Serialize)]
struct WebhookEvent {
    #[serde(rename = "eventType")]
    event_type: String,
    description: String,
}

/// Top-level shape of the `/v1/apis/webhook-event-types` response body.
#[derive(Debug, Deserialize)]
struct WebhookEventsResponse {
    #[serde(default)]
    events: Vec<WebhookEvent>,
}

/// Structured output returned by `webhook events`, matching the TS envelope shape.
#[derive(Serialize)]
struct WebhookEventsOutput {
    events: Vec<WebhookEvent>,
    total: usize,
    shown: usize,
    truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    full_output: Option<String>,
}

const MAX_LIST_ITEMS: usize = 50;

/// Writes the full event list to a temp file and returns its path.
/// Only called when the list is truncated so the caller can offer the full set.
fn write_full_output(events: &[WebhookEvent]) -> Result<String, std::io::Error> {
    let dir = std::env::temp_dir().join(format!("godaddy-cli-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
    }
    let path = dir.join("webhook-events.json");
    let content = serde_json::to_string_pretty(events).map_err(std::io::Error::other)?;
    std::fs::write(&path, content)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(path.to_string_lossy().into_owned())
}

/// Fetches and parses the webhook event-types response.
async fn fetch_webhook_events(
    client: &reqwest::Client,
    base_url: &str,
    token: &str,
) -> cli_engine::Result<WebhookEventsResponse> {
    let url = format!("{base_url}/v1/apis/webhook-event-types");
    let request = client
        .get(&url)
        .bearer_auth(token)
        .header("x-request-id", uuid::Uuid::new_v4().to_string())
        .build()
        .map_err(|e| GddyError::validation(e.to_string()).into_cli_error())?;
    cli_engine::transport::debug_log_reqwest_request(&request);
    let resp = client
        .execute(request)
        .await
        .map_err(|e| GddyError::network(e.to_string()).into_cli_error())?;

    let status = resp.status();
    let headers = resp.headers().clone();
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| GddyError::network(e.to_string()).into_cli_error())?;
    cli_engine::transport::debug_log_reqwest_response(status, &headers, &bytes);

    if !status.is_success() {
        let body = String::from_utf8_lossy(&bytes).into_owned();
        return Err(GddyError::from_http(status.as_u16(), body, "webhooks").into_cli_error());
    }
    serde_json::from_slice(&bytes).map_err(|e| {
        GddyError::unexpected(format!("failed to parse webhook events response: {e}"))
            .into_cli_error()
    })
}

/// The webhook command group, composed below `gddy platform`.
pub fn group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(
        GroupSpec::new("webhook", "Manage webhook event types").with_long(
            "Inspect the webhook event types your application can subscribe to.\n\
                     Use `gddy platform app add subscription` to attach a webhook \
                     subscription to an application.",
        ),
    )
    .with_command(RuntimeCommandSpec::new_with_context(
        CommandSpec::new("events", "List available webhook event types")
            .with_long(
                "Returns available webhook event types. Up to 50 events are shown; \
                         if there are more, the output includes a `full_output` field \
                         with a path to a file containing the complete list.\n\
                         Pass an event type from this list to \
                         `gddy platform app add subscription`.",
            )
            .with_system("webhooks")
            .with_tier(Tier::Read),
        |ctx| async move {
            let token = ctx.credential().await?.token;
            let base_url = api_url_for_env(&ctx.middleware.env)?;
            let client = crate::application::client::make_http_client();
            let response = fetch_webhook_events(&client, &base_url, &token).await?;
            let events: Vec<WebhookEvent> = response.events;
            let total = events.len();
            let truncated = total > MAX_LIST_ITEMS;
            let shown = total.min(MAX_LIST_ITEMS);
            let full_output: Option<String> = if truncated {
                let path = write_full_output(&events).map_err(|e| {
                    GddyError::unexpected(format!("failed to write full output: {e}"))
                        .into_cli_error()
                })?;
                Some(path)
            } else {
                None
            };
            let items: Vec<WebhookEvent> = if truncated {
                events.into_iter().take(MAX_LIST_ITEMS).collect()
            } else {
                events
            };
            let output = WebhookEventsOutput {
                events: items,
                total,
                shown,
                truncated,
                full_output,
            };
            let out = serde_json::to_value(&output).map_err(|e| {
                GddyError::unexpected(format!("failed to serialize output: {e}")).into_cli_error()
            })?;
            Ok(CommandResult::new(out))
        },
    ))
}

#[cfg(test)]
mod tests {
    use httpmock::prelude::*;

    use super::*;

    /// `webhook events` calls the platform API, so it must stay fail-closed
    /// like every other authenticated command (parity with the deleted TS
    /// webhook-service test's "should throw authentication error"/"should
    /// throw error with null access token" cases — here the credential gate
    /// rejects before the handler ever builds a request).
    #[tokio::test]
    async fn webhook_events_requires_auth() {
        let cli = cli_engine::Cli::new(
            cli_engine::CliConfig::new("gddy", "GoDaddy developer CLI", "gddy")
                .with_min_stage(cli_engine::Stage::Experimental)
                .with_module(crate::platform::module()),
        );
        let output = cli
            .run(["gddy", "platform", "webhook", "events", "--output", "json"])
            .await;
        assert_eq!(output.exit_code, 2, "{}", output.rendered);
    }

    // --- fetch_webhook_events: HTTP wiring ---

    #[tokio::test]
    async fn fetch_webhook_events_sends_bearer_token_and_parses_response() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/v1/apis/webhook-event-types")
                    .header("authorization", "Bearer test-token");
                then.status(200).json_body(serde_json::json!({
                    "events": [
                        { "eventType": "application.created", "description": "created" },
                    ]
                }));
            })
            .await;

        let client = crate::application::client::make_http_client();
        let response = fetch_webhook_events(&client, &server.base_url(), "test-token")
            .await
            .expect("should fetch events");

        mock.assert_async().await;
        assert_eq!(response.events.len(), 1);
        assert_eq!(response.events[0].event_type, "application.created");
    }

    #[tokio::test]
    async fn fetch_webhook_events_maps_a_non_2xx_status_to_a_cli_error() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(GET).path("/v1/apis/webhook-event-types");
                then.status(401).body("invalid token");
            })
            .await;

        let client = crate::application::client::make_http_client();
        let err = fetch_webhook_events(&client, &server.base_url(), "bad-token")
            .await
            .expect_err("non-2xx status should surface as an error");

        mock.assert_async().await;
        assert!(
            err.to_string().contains("invalid token"),
            "expected the upstream body in the error: {err}"
        );
    }

    // --- Deserialization ---

    #[test]
    fn webhook_event_maps_camel_case_and_drops_extra_fields() {
        let raw = serde_json::json!({
            "eventType": "domain.created",
            "description": "A domain was created",
            "extraField": "should be ignored"
        });
        let event: WebhookEvent = serde_json::from_value(raw).expect("should deserialize");
        assert_eq!(event.event_type, "domain.created");
        assert_eq!(event.description, "A domain was created");
    }

    #[test]
    fn webhook_events_response_defaults_events_to_empty_when_key_absent() {
        let raw = serde_json::json!({});
        let resp: WebhookEventsResponse = serde_json::from_value(raw).expect("should deserialize");
        assert!(resp.events.is_empty());
    }

    // --- Output serialization ---

    #[test]
    fn output_omits_full_output_field_when_not_truncated() {
        let output = WebhookEventsOutput {
            events: vec![],
            total: 3,
            shown: 3,
            truncated: false,
            full_output: None,
        };
        let json = serde_json::to_value(&output).expect("should serialize");
        assert!(
            json.get("full_output").is_none(),
            "full_output should be absent when None, got: {json}"
        );
    }

    #[test]
    fn output_includes_full_output_and_metadata_when_truncated() {
        let output = WebhookEventsOutput {
            events: vec![],
            total: 100,
            shown: 50,
            truncated: true,
            full_output: Some("/tmp/godaddy-cli/test.json".to_string()),
        };
        let json = serde_json::to_value(&output).expect("should serialize");
        assert_eq!(json["total"], 100);
        assert_eq!(json["shown"], 50);
        assert_eq!(json["truncated"], true);
        assert_eq!(json["full_output"], "/tmp/godaddy-cli/test.json");
    }

    // --- Temp file writing ---

    #[test]
    fn write_full_output_creates_file_with_camel_case_json() {
        let events = vec![WebhookEvent {
            event_type: "domain.created".to_string(),
            description: "A domain was created".to_string(),
        }];
        let path = write_full_output(&events).expect("should write temp file");
        let content = std::fs::read_to_string(&path).expect("should read temp file");
        let _ = std::fs::remove_file(&path);
        let parsed: serde_json::Value =
            serde_json::from_str(&content).expect("file should contain valid JSON");
        assert_eq!(parsed[0]["eventType"], "domain.created");
        assert_eq!(parsed[0]["description"], "A domain was created");
    }
}
