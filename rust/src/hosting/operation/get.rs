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
         FAILED. The `type` field says which kind of operation it is, CREATE_APP or \
         SET_PUBLISH_STATE; an operation with no `type` is a create. On COMPLETED, a \
         create's `app` field carries the created application; use `app.id` for \
         subsequent calls. On FAILED, the `error` field describes why."
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

/// True unless the operation says it is something other than a creation.
/// `type` is optional on the wire — a service that predates it sends none, and
/// an operation without one is a creation — so only an explicit non-create
/// `type` rules a creation out.
fn is_create(data: &Value) -> bool {
    data["type"]
        .as_str()
        .is_none_or(|kind| kind == "CREATE_APP")
}

/// Suggests a retry when a create failed for lack of capacity. A failed
/// publish-state change is not retried this way: re-running the create would
/// build a second site, and the operation carries no app id to publish again.
fn next_actions(data: &Value) -> Vec<NextAction> {
    let no_capacity = data["status"] == "FAILED"
        && is_create(data)
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
        // Once with no `type` (the services that predate the field) and once
        // with the explicit creation `type`: both are creations.
        for kind in [None, Some("CREATE_APP")] {
            for issue in ["NO_CAPACITY", "CAPACITY_UNAVAILABLE"] {
                let mut data = json!({
                    "operationId": "MHWP-ab12cd34",
                    "status": "FAILED",
                    "error": { "message": "No capacity.", "details": [{ "issue": issue }] },
                    "links": []
                });
                if let Some(kind) = kind {
                    data["type"] = json!(kind);
                }
                let actions = next_actions(&data);
                assert_eq!(actions.len(), 1, "{kind:?}/{issue}");
                assert!(
                    actions[0].command.contains("hosting app create"),
                    "{kind:?}/{issue}"
                );
                assert!(
                    actions[0].description.contains("Try again later"),
                    "{kind:?}/{issue}"
                );
                assert_eq!(actions[0].params["type"].value.as_deref(), Some("MHWP"));
            }
        }
    }

    /// A failed publish-state change must not be answered with "create the app
    /// again" — the retry only makes sense for a creation.
    #[test]
    fn suggests_nothing_for_a_failed_publish_state_change() {
        let actions = next_actions(&json!({
            "operationId": "MHWP-ab12cd34",
            "type": "SET_PUBLISH_STATE",
            "status": "FAILED",
            "error": {
                "message": "No capacity.",
                "details": [{ "issue": "CAPACITY_UNAVAILABLE" }]
            },
            "links": []
        }));
        assert!(actions.is_empty(), "{actions:?}");
    }

    /// `type` is informational for the caller, so it reaches both the JSON and
    /// the human rendering unchanged, present or absent.
    #[test]
    fn the_operation_type_is_carried_through_to_the_output() {
        let with_type = json!({
            "operationId": "MHWP-ab12cd34",
            "type": "SET_PUBLISH_STATE",
            "status": "IN_PROGRESS",
            "links": []
        });
        let rendered =
            cli_engine::render_human(&cli_engine::Envelope::success(with_type.clone(), "hosting"));
        assert!(rendered.contains("SET_PUBLISH_STATE"), "{rendered}");
        assert!(rendered.contains("type"), "{rendered}");

        // Absent on an older service's response: nothing is invented for it.
        let without_type = json!({
            "operationId": "MHWP-ab12cd34",
            "status": "IN_PROGRESS",
            "links": []
        });
        let rendered =
            cli_engine::render_human(&cli_engine::Envelope::success(without_type, "hosting"));
        assert!(!rendered.contains("type"), "{rendered}");
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
