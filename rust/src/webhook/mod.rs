use cli_engine::{
    CommandResult, CommandSpec, GroupSpec, RuntimeCommandSpec, RuntimeGroupSpec, Tier,
};

use crate::application::client::api_url_for_env;

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
                "Returns the full list of event types that can be used when \
                         creating a webhook subscription.\n\
                         Pass an event type from this list to \
                         `gddy platform app add subscription`.",
            )
            .with_system("webhooks")
            .with_tier(Tier::Read),
        |ctx| async move {
            let token = ctx.credential().await?.token;
            let base_url = api_url_for_env(&ctx.middleware.env);
            let url = format!("{base_url}/v1/apis/webhook-event-types");
            let client = crate::application::client::make_http_client();
            let request = client
                .get(&url)
                .bearer_auth(&token)
                .header("x-request-id", uuid::Uuid::new_v4().to_string())
                .build()
                .map_err(|e| cli_engine::CliCoreError::message(e.to_string()))?;
            cli_engine::transport::debug_log_reqwest_request(&request);
            let resp = client
                .execute(request)
                .await
                .map_err(|e| cli_engine::CliCoreError::message(e.to_string()))?;

            let status = resp.status();
            let headers = resp.headers().clone();
            let bytes = resp
                .bytes()
                .await
                .map_err(|e| cli_engine::CliCoreError::message(e.to_string()))?;
            cli_engine::transport::debug_log_reqwest_response(status, &headers, &bytes);

            if !status.is_success() {
                let body = String::from_utf8_lossy(&bytes).into_owned();
                return Err(cli_engine::CliCoreError::message(format!(
                    "webhook events request failed ({}): {body}",
                    status.as_u16()
                )));
            }
            let data: serde_json::Value = serde_json::from_slice(&bytes)
                .map_err(|e| cli_engine::CliCoreError::message(e.to_string()))?;
            Ok(CommandResult::new(data))
        },
    ))
}
