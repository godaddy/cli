//! Shared start, stop and restart flow: confirm gate, request, then wait.
use std::future::Future;
use std::time::Duration;

use cli_engine::{CommandContext, CommandResult, NextActionParam};
use serde_json::Value;

use crate::error::GddyError;
use crate::hosting::client::ClientError;
use crate::hosting::common::{client_err, make_client};
use crate::next_action::next_action;
use crate::scopes::{HOSTING_SERVER_EXECUTE as SERVER_EXECUTE, HOSTING_SERVER_READ as SERVER_READ};

const POLL_EVERY: Duration = Duration::from_secs(5);
const GIVE_UP_AFTER: Duration = Duration::from_secs(15 * 60);

#[derive(Debug, Clone, Copy)]
pub(super) enum Power {
    Start,
    Stop,
    Restart,
}

impl Power {
    fn path(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Stop => "stop",
            Self::Restart => "restart",
        }
    }

    /// Why the action needs `--confirm`; `None` when it does not.
    pub(super) fn warning(self) -> Option<&'static str> {
        match self {
            Self::Start => None,
            Self::Stop => {
                Some("stopping this server takes it, and everything running on it, offline.")
            }
            Self::Restart => Some("restarting this server takes it offline for a short time."),
        }
    }
}

/// Stop and restart send nothing without `--confirm`.
pub(super) fn require_confirm(power: Power, confirm: bool) -> cli_engine::Result<()> {
    match power.warning() {
        Some(warning) if !confirm => Err(GddyError::validation(format!(
            "{warning}\nRe-run with --confirm to proceed."
        ))
        .into_cli_error()),
        _ => Ok(()),
    }
}

pub(super) async fn run(
    ctx: &CommandContext,
    power: Power,
    server_id: String,
    confirm: bool,
    no_wait: bool,
) -> cli_engine::Result<CommandResult> {
    require_confirm(power, confirm)?;
    let client = make_client(ctx, &[SERVER_EXECUTE, SERVER_READ]).await?;
    let started = client
        .server_action(&server_id, power.path())
        .await
        .map_err(client_err)?;
    if no_wait {
        return Ok(outcome(&server_id, started, power));
    }
    let operation_id = started["operationId"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let done = wait_until_done(
        started,
        || client.get_server_operation(&server_id, &operation_id),
        POLL_EVERY,
        GIVE_UP_AFTER,
    )
    .await
    .map_err(|e| match e {
        WaitError::Client(e) => client_err(e),
        WaitError::TimedOut => timed_out(&server_id, &operation_id),
    })?;
    if done["status"] == "FAILED" {
        return Err(failed(&server_id, &done));
    }
    Ok(outcome(&server_id, done, power))
}

pub(super) enum WaitError {
    Client(ClientError),
    TimedOut,
}

/// Polls `fetch` until the operation is COMPLETED or FAILED.
pub(super) async fn wait_until_done<F, Fut>(
    first: Value,
    mut fetch: F,
    every: Duration,
    give_up_after: Duration,
) -> Result<Value, WaitError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<Value, ClientError>>,
{
    let started = std::time::Instant::now();
    let mut current = first;
    while !is_finished(&current) {
        if started.elapsed() >= give_up_after {
            return Err(WaitError::TimedOut);
        }
        tokio::time::sleep(every).await;
        current = fetch().await.map_err(WaitError::Client)?;
    }
    Ok(current)
}

pub(super) fn is_finished(operation: &Value) -> bool {
    matches!(operation["status"].as_str(), Some("COMPLETED" | "FAILED"))
}

fn timed_out(server_id: &str, operation_id: &str) -> cli_engine::CliCoreError {
    GddyError::unexpected("still running after 15 minutes; check with operation get")
        .with_fix(format!(
            "Run: gddy hosting vps operation get --server-id {server_id} --operation-id {operation_id}"
        ))
        .into_cli_error()
}

fn failed(server_id: &str, operation: &Value) -> cli_engine::CliCoreError {
    let reason = operation["error"]["details"][0]["issue"]
        .as_str()
        .or_else(|| operation["error"]["message"].as_str())
        .unwrap_or("unknown");
    GddyError::unexpected(format!(
        "operation {} ({}) failed: {reason}",
        operation["operationId"].as_str().unwrap_or_default(),
        operation["type"].as_str().unwrap_or_default(),
    ))
    .with_fix(format!(
        "Check the server with: gddy hosting vps get --server-id {server_id}, then retry"
    ))
    .into_cli_error()
}

fn outcome(server_id: &str, operation: Value, power: Power) -> CommandResult {
    let actions = if is_finished(&operation) {
        let (description, command) = match power {
            Power::Stop => (
                "Start the server again",
                "hosting vps start --server-id <server-id>",
            ),
            _ => ("Show the server", "hosting vps get --server-id <server-id>"),
        };
        vec![
            next_action(command, description)
                .with_param("server-id", NextActionParam::value(server_id)),
        ]
    } else {
        vec![
            next_action(
                "hosting vps operation get --server-id <server-id> --operation-id <operation-id>",
                "Check the operation until it finishes",
            )
            .with_param("server-id", NextActionParam::value(server_id))
            .with_param(
                "operation-id",
                NextActionParam::value(operation["operationId"].as_str().unwrap_or_default()),
            ),
        ]
    };
    CommandResult::new(operation).with_next_actions(actions)
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use serde_json::json;

    use super::*;

    #[test]
    fn stop_and_restart_need_confirm_but_start_does_not() {
        assert!(require_confirm(Power::Start, false).is_ok());
        for power in [Power::Stop, Power::Restart] {
            assert!(require_confirm(power, false).is_err());
            assert!(require_confirm(power, true).is_ok());
        }
    }

    #[tokio::test]
    async fn waits_until_the_operation_completes() {
        let polls = Cell::new(0);
        let done = wait_until_done(
            json!({ "operationId": "1", "status": "PENDING" }),
            || {
                polls.set(polls.get() + 1);
                let status = if polls.get() < 3 {
                    "IN_PROGRESS"
                } else {
                    "COMPLETED"
                };
                async move { Ok(json!({ "operationId": "1", "status": status })) }
            },
            Duration::ZERO,
            Duration::from_secs(60),
        )
        .await
        .map_err(|_| "wait failed")
        .expect("completes");
        assert_eq!(done["status"], "COMPLETED");
        assert_eq!(polls.get(), 3);
    }

    #[tokio::test]
    async fn a_finished_operation_is_not_polled() {
        let done = wait_until_done(
            json!({ "status": "COMPLETED" }),
            || async { Err(ClientError::Request("must not poll".to_owned())) },
            Duration::ZERO,
            Duration::ZERO,
        )
        .await
        .map_err(|_| "polled a finished operation")
        .expect("no poll");
        assert_eq!(done["status"], "COMPLETED");
    }

    #[tokio::test]
    async fn gives_up_when_it_never_finishes() {
        let result = wait_until_done(
            json!({ "status": "PENDING" }),
            || async { Ok(json!({ "status": "IN_PROGRESS" })) },
            Duration::ZERO,
            Duration::ZERO,
        )
        .await;
        assert!(matches!(result, Err(WaitError::TimedOut)));
    }

    #[test]
    fn a_pending_operation_points_at_operation_get() {
        let result = outcome(
            "VPS-phx3-a",
            json!({ "operationId": "9", "status": "PENDING" }),
            Power::Stop,
        );
        assert!(
            result.metadata.next_actions[0]
                .command
                .contains("operation get")
        );
    }

    #[test]
    fn a_finished_stop_offers_start() {
        let result = outcome(
            "VPS-phx3-a",
            json!({ "operationId": "9", "status": "COMPLETED" }),
            Power::Stop,
        );
        assert!(
            result.metadata.next_actions[0]
                .command
                .contains("vps start")
        );
    }
}
