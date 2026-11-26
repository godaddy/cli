use cli_engine::{
    CommandResult, CommandSpec, NextAction, NextActionParam, RuntimeCommandSpec, Tier,
};
use serde_json::Value;

use crate::hosting::common::{HostingServerOperation, client_err, make_client};
use crate::next_action::next_action;
use crate::scopes::HOSTING_SERVER_READ as SERVER_READ;

#[derive(Debug, Clone, clap::Args)]
struct OperationGetArgs {
    /// Server ID, as shown by `hosting vps list`.
    #[arg(long = "server-id", value_name = "SERVER_ID")]
    server_id: String,

    /// Operation ID returned by start, stop or restart.
    #[arg(long = "operation-id", value_name = "OPERATION_ID")]
    operation_id: String,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<OperationGetArgs, _, _, _>(
        CommandSpec::from_args::<OperationGetArgs>("get", "Show the state of a server operation")
            .with_long(
                "Show the state of an operation started by `hosting vps start`, `stop` or \
                 `restart`: PENDING, IN_PROGRESS, COMPLETED or FAILED. A FAILED operation \
                 carries the reason.",
            )
            .with_system("hosting")
            .with_tier(Tier::Read)
            .with_scopes(&[SERVER_READ])
            .with_output_schema::<HostingServerOperation>(),
        |ctx, args: OperationGetArgs| async move {
            let client = make_client(&ctx, &[SERVER_READ]).await?;
            let data = client
                .get_server_operation(&args.server_id, &args.operation_id)
                .await
                .map_err(client_err)?;
            let actions = next_actions(&data, &args.server_id, &args.operation_id);
            Ok(CommandResult::new(data).with_next_actions(actions))
        },
    )
}

fn next_actions(data: &Value, server_id: &str, operation_id: &str) -> Vec<NextAction> {
    let server = NextActionParam::value(server_id);
    if super::super::power::is_finished(data) {
        let description = if data["status"] == "FAILED" {
            "Check the server state, then retry the action"
        } else {
            "Show the server"
        };
        return vec![
            next_action("hosting vps get --server-id <server-id>", description)
                .with_param("server-id", server),
        ];
    }
    vec![
        next_action(
            "hosting vps operation get --server-id <server-id> --operation-id <operation-id>",
            "Still running. Check again in a few seconds",
        )
        .with_param("server-id", server)
        .with_param("operation-id", NextActionParam::value(operation_id)),
    ]
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_running_operation_points_back_at_itself() {
        let actions = next_actions(&json!({ "status": "IN_PROGRESS" }), "VPS-phx3-a", "7");
        assert!(actions[0].command.contains("operation get"));
    }

    #[test]
    fn a_finished_operation_points_at_the_server() {
        for status in ["COMPLETED", "FAILED"] {
            let actions = next_actions(&json!({ "status": status }), "VPS-phx3-a", "7");
            assert!(actions[0].command.contains("vps get"), "{status}");
        }
    }
}
