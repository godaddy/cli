use cli_engine::{
    CommandResult, CommandSpec, GroupSpec, Module, RuntimeCommandSpec, RuntimeGroupSpec, Tier,
};
use serde_json::json;

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
fn load_action_schema(name: &str) -> Option<serde_json::Value> {
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

pub fn module() -> Module {
    Module::new("Contracts", |_ctx| {
        RuntimeGroupSpec::new(GroupSpec::new(
            "actions",
            "Discover available GoDaddy action contracts",
        ))
        .with_command(RuntimeCommandSpec::new(
            CommandSpec::new("list", "List all available action contracts")
                .with_system("actions")
                .with_tier(Tier::Read),
            |_cred, _args| async move {
                let actions: Vec<_> = ACTIONS
                    .iter()
                    .map(|(name, description)| json!({ "name": name, "description": description }))
                    .collect();
                Ok(CommandResult::new(json!({ "actions": actions })))
            },
        ))
        .with_command(RuntimeCommandSpec::new_with_context(
            CommandSpec::new("describe", "Show detailed schema for an action contract")
                .with_system("actions")
                .with_tier(Tier::Read)
                .with_arg(
                    clap::Arg::new("action")
                        .value_name("ACTION")
                        .required(true)
                        .help("Action name (e.g. location.address.verify)"),
                ),
            |ctx| async move {
                let name = ctx
                    .args
                    .get("action")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_owned();
                let schema = load_action_schema(&name).ok_or_else(|| {
                    cli_engine::CliCoreError::message(format!(
                        "action {name:?} not found; run `godaddy actions list` to see available actions"
                    ))
                })?;
                Ok(CommandResult::new(schema))
            },
        ))
    })
}
