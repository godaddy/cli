use cli_engine::{
    CommandResult, CommandSpec, NextAction, NextActionParam, RuntimeCommandSpec, Tier,
};
use serde_json::Value;

use crate::hosting::common::{HostingAppOperation, HostingAppType, client_err, make_client};
use crate::next_action::next_action;
use crate::scopes::HOSTING_APPLICATION_READ as APP_READ;

#[derive(Debug, Clone, clap::Args)]
struct OperationGetArgs {
    /// Operation ID.
    #[arg(long = "operation-id", value_name = "OPERATION_ID")]
    operation_id: String,
}

pub(super) fn command(mhwp: bool) -> RuntimeCommandSpec {
    // Only WordPress sites have a publish state; name it only while the flag is on.
    let long = if mhwp {
        "Poll an async operation by ID. `hosting app create`, `hosting app publish` and \
         `hosting app unpublish` return one — `hosting deployment publish` has its own \
         poller at `hosting deployment get`. Keep polling until status is COMPLETED or \
         FAILED. On COMPLETED, a create's `app` field carries the created application; \
         use `app.id` for subsequent calls. On FAILED, the `error` field describes why."
    } else {
        "Poll an async operation by ID. Today only `hosting app create` returns one — \
         `hosting deployment publish` has its own poller at `hosting deployment get`. \
         Keep polling until status is COMPLETED or FAILED. On COMPLETED, the `app` field \
         carries the created application; use `app.id` for subsequent calls. On FAILED, \
         the `error` field describes why."
    };
    RuntimeCommandSpec::new_typed_with_context::<OperationGetArgs, _, _, _>(
        CommandSpec::from_args::<OperationGetArgs>("get", "Get an operation")
            .with_long(long)
            .with_system("hosting")
            .with_tier(Tier::Read)
            .with_scopes(&[APP_READ])
            .with_output_schema::<HostingAppOperation>(),
        |ctx, args: OperationGetArgs| async move {
            let client = make_client(&ctx, &[APP_READ]).await?;
            let data = client
                .get_operation(&args.operation_id)
                .await
                .map_err(client_err)?;
            let next_actions = next_actions(&data);
            Ok(CommandResult::new(data).with_next_actions(next_actions))
        },
    )
}

/// Issue codes a failed create carries when no capacity was available:
/// `NO_CAPACITY` (Node.js) and `CAPACITY_UNAVAILABLE` (WordPress).
const NO_CAPACITY_ISSUES: [&str; 2] = ["NO_CAPACITY", "CAPACITY_UNAVAILABLE"];

/// Suggests a retry when a create failed for lack of capacity.
fn next_actions(data: &Value) -> Vec<NextAction> {
    let no_capacity = data["status"] == "FAILED"
        && data["error"]["details"].as_array().is_some_and(|details| {
            details.iter().any(|d| {
                d["issue"]
                    .as_str()
                    .is_some_and(|issue| NO_CAPACITY_ISSUES.contains(&issue))
            })
        });
    if !no_capacity {
        return Vec::new();
    }
    // MHWP operation ids carry the app type as a prefix, e.g. `MHWP-ab12cd34`.
    let app_type = data["operationId"]
        .as_str()
        .and_then(|id| id.split_once('-'))
        .and_then(|(prefix, _)| clap::ValueEnum::from_str(prefix, true).ok())
        .map_or_else(NextActionParam::required, |t: HostingAppType| {
            NextActionParam::value(t.as_str())
        });
    vec![
        next_action(
            "hosting app create --app-type <type> --name <name>",
            "No capacity is available right now. Try again later",
        )
        .with_param("type", app_type)
        .with_param("name", NextActionParam::required()),
    ]
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn suggests_a_retry_on_no_capacity() {
        for issue in ["NO_CAPACITY", "CAPACITY_UNAVAILABLE"] {
            let actions = next_actions(&json!({
                "operationId": "MHWP-ab12cd34",
                "status": "FAILED",
                "error": { "message": "No capacity.", "details": [{ "issue": issue }] },
                "links": []
            }));
            assert_eq!(actions.len(), 1, "{issue}");
            assert!(actions[0].command.contains("hosting app create"), "{issue}");
            assert!(
                actions[0].description.contains("Try again later"),
                "{issue}"
            );
            assert_eq!(actions[0].params["type"].value.as_deref(), Some("MHWP"));
        }
    }

    #[test]
    fn suggests_nothing_for_other_outcomes() {
        assert!(next_actions(&json!({ "status": "IN_PROGRESS", "links": [] })).is_empty());
        assert!(
            next_actions(&json!({
                "status": "FAILED",
                "error": { "message": "boom", "details": [{ "issue": "INTERNAL" }] }
            }))
            .is_empty()
        );
    }

    #[test]
    fn leaves_the_type_open_for_unprefixed_ids() {
        let unprefixed = next_actions(&json!({
            "operationId": "3f2a9c1e-77d0-4b1a-9c55-0e6f1a2b3c4d",
            "status": "FAILED",
            "error": { "message": "No capacity.", "details": [{ "issue": "NO_CAPACITY" }] }
        }));
        assert_eq!(unprefixed[0].params["type"].value, None);
    }
}
