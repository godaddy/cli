use cli_engine::{CommandResult, CommandSpec, NextActionParam, RuntimeCommandSpec, Tier};
use serde_json::json;

use crate::hosting::nodejs::{client_err, make_client};
use crate::next_action::next_action;
use crate::scopes::HOSTING_APPS_CREATE as APPS_CREATE;

#[derive(Debug, Clone, clap::Args)]
struct AppCreateArgs {
    /// Application name.
    #[arg(long, value_name = "NAME")]
    name: String,

    /// Datacenter (p3 or sxb1).
    #[arg(long, value_name = "DATACENTER", value_parser = ["p3", "sxb1"])]
    datacenter: Option<String>,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<AppCreateArgs, _, _, _>(
        CommandSpec::from_args::<AppCreateArgs>("create", "Create a Node.js hosting application")
            .with_long(
                "One-time provisioning of a new app slot. Do not call this when pushing \
                 code to an app that already exists — use `source upload` (zip) or \
                 `source git` (GitHub) instead. Returns a creation job; poll `job get` \
                 until status is `active`.",
            )
            .with_system("hosting")
            .with_tier(Tier::Mutate)
            .mutates(true)
            .with_scopes(&[APPS_CREATE]),
        |ctx, args: AppCreateArgs| async move {
            let name = args.name;
            let mut body = json!({ "name": name });
            if let Some(datacenter) = args.datacenter {
                body["datacenter"] = json!(datacenter);
            }
            let client = make_client(&ctx, &[APPS_CREATE]).await?;
            let data = client.create_app(body).await.map_err(client_err)?;
            let job_id = data
                .pointer("/job/id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let mut actions = vec![
                next_action(
                    "hosting nodejs job get --job-id <job-id>",
                    "Poll app creation status",
                )
                .with_param("job-id", NextActionParam::required()),
            ];
            if !job_id.is_empty() {
                actions[0] = next_action(
                    "hosting nodejs job get --job-id <job-id>",
                    "Poll app creation status",
                )
                .with_param("job-id", NextActionParam::value(job_id));
            }
            Ok(CommandResult::new(data).with_next_actions(actions))
        },
    )
}
