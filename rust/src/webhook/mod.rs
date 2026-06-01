use cli_engine::{
    CommandResult, CommandSpec, GroupSpec, Module, RuntimeCommandSpec, RuntimeGroupSpec, Tier,
};

use crate::application::client::api_url_for_env;

pub fn module() -> Module {
    Module::new("GPA", |_ctx| {
        RuntimeGroupSpec::new(GroupSpec::new("webhook", "Manage webhook event types")).with_command(
            RuntimeCommandSpec::new_with_context(
                CommandSpec::new("events", "List available webhook event types")
                    .with_system("webhooks")
                    .with_tier(Tier::Read),
                |ctx| async move {
                    let token = ctx
                        .credential
                        .as_ref()
                        .map(|c| c.token.clone())
                        .unwrap_or_default();
                    let base_url = api_url_for_env(&ctx.middleware.env);
                    let url = format!("{base_url}/v1/apis/webhook-event-types");
                    let resp = crate::application::client::make_http_client()
                        .get(&url)
                        .bearer_auth(&token)
                        .header("x-request-id", uuid::Uuid::new_v4().to_string())
                        .send()
                        .await
                        .map_err(|e| cli_engine::CliCoreError::message(e.to_string()))?;
                    if !resp.status().is_success() {
                        let status = resp.status().as_u16();
                        let body = resp.text().await.unwrap_or_default();
                        return Err(cli_engine::CliCoreError::message(format!(
                            "webhook events request failed ({status}): {body}"
                        )));
                    }
                    let data: serde_json::Value = resp
                        .json()
                        .await
                        .map_err(|e| cli_engine::CliCoreError::message(e.to_string()))?;
                    Ok(CommandResult::new(data))
                },
            ),
        )
    })
}
