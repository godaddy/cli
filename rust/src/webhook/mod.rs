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
            let url = format!("{base_url}/v1/apis/webhook-event-types");
            let client = crate::application::client::make_http_client();
            let request = client
                .get(&url)
                .bearer_auth(&token)
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
                return Err(
                    GddyError::from_http(status.as_u16(), body, "webhooks").into_cli_error()
                );
            }
            let response: WebhookEventsResponse = serde_json::from_slice(&bytes).map_err(|e| {
                GddyError::unexpected(format!("failed to parse webhook events response: {e}"))
                    .into_cli_error()
            })?;
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
    use super::*;

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
