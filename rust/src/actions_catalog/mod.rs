use cli_engine::{
    CommandResult, CommandSpec, GroupSpec, NextActionParam, PaginationConfig, RuntimeCommandSpec,
    RuntimeGroupSpec, Tier,
};
use serde_json::{Value, json};

use crate::next_action::next_action;
use crate::output_schema::output_schema;
use crate::truncation::protect_payload;

output_schema!(ActionSummary {
    "name": "string";
    "description": "string";
});

/// Static catalog of available GoDaddy action names.
/// Loaded from the manifest at compile time.
const ACTIONS: &[(&str, &str)] = &[
    (
        "location.address.verify",
        "Verify and standardize a physical address",
    ),
    (
        "commerce.taxes.calculate",
        "Calculate taxes for a commerce transaction",
    ),
    (
        "commerce.shipping-rates.calculate",
        "Calculate shipping rates",
    ),
    (
        "commerce.price-adjustment.apply",
        "Apply a price adjustment",
    ),
    ("notifications.email.send", "Send an email notification"),
    ("commerce.payment.process", "Process a payment"),
    ("commerce.payment.refund", "Refund a payment"),
];

/// Raw action schema JSON files embedded at compile time.
/// Key is the action name; value is the full JSON schema.
fn load_action_schema(name: &str) -> Option<Value> {
    let json_str = match name {
        "location.address.verify" => {
            include_str!("../../schemas/actions/location-address-verify.json")
        }
        "commerce.taxes.calculate" => {
            include_str!("../../schemas/actions/commerce-taxes-calculate.json")
        }
        "commerce.shipping-rates.calculate" => {
            include_str!("../../schemas/actions/commerce-shipping-rates-calculate.json")
        }
        "commerce.price-adjustment.apply" => {
            include_str!("../../schemas/actions/commerce-price-adjustment-apply.json")
        }
        "notifications.email.send" => {
            include_str!("../../schemas/actions/notifications-email-send.json")
        }
        "commerce.payment.process" => {
            include_str!("../../schemas/actions/commerce-payment-process.json")
        }
        "commerce.payment.refund" => {
            include_str!("../../schemas/actions/commerce-payment-refund.json")
        }
        _ => return None,
    };
    serde_json::from_str(json_str).ok()
}

#[derive(Debug, Clone, clap::Args)]
struct DescribeArgs {
    /// Name of the action to describe (e.g. location.address.verify).
    #[arg(value_name = "ACTION")]
    action: String,
}

/// The action-catalog command group, composed below `gddy platform`.
pub fn group() -> RuntimeGroupSpec {
    RuntimeGroupSpec::new(
        GroupSpec::new("actions", "Discover available GoDaddy action contracts")
            .with_long(
                "Browse the catalog of GoDaddy actions your application can declare.\n\
                     An action contract defines the name, and the input/output schema, \
                     of a capability your app exposes to the GoDaddy platform.\n\
                     Use `gddy platform actions list` to see all available actions, and \
                     `gddy platform actions describe` to inspect a specific action's schema.",
            ),
    )
        .with_command(RuntimeCommandSpec::new(
            CommandSpec::new("list", "List all available action contracts")
                .with_long(
                    "Prints the name and short description of every action your \
                     application can declare. Shows up to 50 actions by default; \
                     use --limit/--offset to page through the rest once the \
                     catalog grows past that.\n\
                     Run `gddy platform actions describe <ACTION>` to see the full \
                     input/output schema for a specific action.",
                )
                .with_system("actions")
                .with_tier(Tier::Read)
                .no_auth(true)
                .with_default_fields("name,description")
                .with_output_schema::<ActionSummary>()
                .with_pagination(PaginationConfig {
                    default_limit: 50,
                    max_limit: 200,
                }),
            |_cred, _args| async move {
                let actions: Vec<_> = ACTIONS
                    .iter()
                    .map(|(name, description)| json!({ "name": name, "description": description }))
                    .collect();
                Ok(CommandResult::new(json!(actions)).with_next_actions(vec![
                    next_action(
                        "platform actions describe <action>",
                        "See an action's full schema",
                    )
                        .with_param("action", NextActionParam::required()),
                ]))
            },
        ))
        .with_command(RuntimeCommandSpec::new_typed::<DescribeArgs, _, _, _>(
            CommandSpec::from_args::<DescribeArgs>(
                "describe",
                "Show the input/output schema for an action",
            )
            .with_long(
                "Prints the full JSON schema for the named action contract, \
                     including its expected input fields and the shape of its output.\n\
                     Run `gddy platform actions list` for the list of valid action names.",
            )
            .with_system("actions")
            .with_tier(Tier::Read)
            .no_auth(true),
            |_cred, args: DescribeArgs| async move {
                let name = args.action;
                let schema = load_action_schema(&name).ok_or_else(|| {
                    cli_engine::CliCoreError::message(format!(
                        "action {name:?} not found; run `gddy platform actions list` to see available actions"
                    ))
                })?;
                let result = protect_payload(schema, &format!("actions-describe-{name}"));
                let mut payload = result.value;
                if let Value::Object(ref mut map) = payload {
                    map.insert("truncated".to_string(), json!(result.metadata.is_some()));
                    if let Some(metadata) = result.metadata {
                        map.insert("total".to_string(), json!(metadata.total));
                        map.insert("shown".to_string(), json!(metadata.shown));

                        // Truncation should always produce a full_output file, but the write
                        // is best-effort and can fail (e.g. disk full, permission denied), so
                        // this stays an `if let` rather than an unconditional insert.
                        if let Some(full_output) = metadata.full_output {
                            map.insert("full_output".to_string(), json!(full_output));
                        }
                    }
                }
                Ok(CommandResult::new(payload))
            },
        ))
}

#[cfg(test)]
mod tests {
    use cli_engine::{Cli, CliConfig, Stage};

    fn test_cli() -> Cli {
        Cli::new(
            CliConfig::new("gddy", "GoDaddy developer CLI", "gddy")
                .with_min_stage(Stage::Experimental)
                .with_modules(crate::all_modules()),
        )
    }

    #[tokio::test]
    async fn list_reports_truncation_metadata_and_no_full_output_under_the_cap() {
        let cli = test_cli();
        let output = cli
            .run(["gddy", "platform", "actions", "list", "--output", "json"])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let rendered: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("stdout should contain json");
        let data = &rendered["data"];
        assert_eq!(data["truncated"], serde_json::json!(false));
        assert_eq!(data["total"], data["shown"]);
        assert!(data.get("full_output").is_none());
    }

    #[tokio::test]
    async fn describe_reports_protection_metadata() {
        let cli = test_cli();
        let output = cli
            .run([
                "gddy",
                "platform",
                "actions",
                "describe",
                "location.address.verify",
                "--output",
                "json",
            ])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let rendered: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("stdout should contain json");
        assert_eq!(rendered["data"]["truncated"], serde_json::json!(false));
    }
}
