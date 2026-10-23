use cli_engine::{
    CommandResult, CommandSpec, CursorConfig, GroupSpec, NextActionParam, RuntimeCommandSpec,
    RuntimeGroupSpec, Tier,
};
use serde_json::{Value, json};

use crate::next_action::next_action;
use crate::output_schema::output_schema;
use crate::pagination_token;
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
            include_str!("../../../schemas/actions/location-address-verify.json")
        }
        "commerce.taxes.calculate" => {
            include_str!("../../../schemas/actions/commerce-taxes-calculate.json")
        }
        "commerce.shipping-rates.calculate" => {
            include_str!("../../../schemas/actions/commerce-shipping-rates-calculate.json")
        }
        "commerce.price-adjustment.apply" => {
            include_str!("../../../schemas/actions/commerce-price-adjustment-apply.json")
        }
        "notifications.email.send" => {
            include_str!("../../../schemas/actions/notifications-email-send.json")
        }
        "commerce.payment.process" => {
            include_str!("../../../schemas/actions/commerce-payment-process.json")
        }
        "commerce.payment.refund" => {
            include_str!("../../../schemas/actions/commerce-payment-refund.json")
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
        GroupSpec::new("action", "Discover available GoDaddy action contracts")
            .with_alias("actions")
            .with_long(
                "Browse the catalog of GoDaddy actions your application can declare.\n\
                     An action contract defines the name, and the input/output schema, \
                     of a capability your app exposes to the GoDaddy platform.\n\
                     Use `gddy platform action list` to see all available actions, and \
                     `gddy platform action describe` to inspect a specific action's schema.",
            ),
    )
        .with_command(RuntimeCommandSpec::new_with_context(
            CommandSpec::new("list", "List all available action contracts")
                .with_long(
                    "Prints the name and short description of every action your \
                     application can declare. Shows up to 50 actions by default; \
                     use --limit/--continue to page through the rest once the \
                     catalog grows past that.\n\
                     Run `gddy platform action describe <ACTION>` to see the full \
                     input/output schema for a specific action.",
                )
                .with_system("actions")
                .with_tier(Tier::Read)
                .no_auth(true)
                .with_default_fields("name,description")
                .with_output_schema::<ActionSummary>()
                .with_cursor(CursorConfig::new(50, 200))
                .with_arg(
                    clap::Arg::new("offset")
                        .long("offset")
                        .value_name("N")
                        .value_parser(clap::value_parser!(i64).range(0..))
                        .help(
                            "Jump directly to an offset in the results. \
                             Ignored when `--continue` is given.",
                        ),
                ),
            |ctx| async move {
                let actions: Vec<_> = ACTIONS
                    .iter()
                    .map(|(name, description)| json!({ "name": name, "description": description }))
                    .collect();
                let explicit_offset = ctx.args.get("offset").and_then(Value::as_i64);
                let continue_token = pagination_token::resolve_continue_token(
                    explicit_offset,
                    ctx.middleware.continue_token.as_deref(),
                );
                let (page, continuation) = pagination_token::paginate_known_collection(
                    pagination_token::filter_known_collection(actions, &ctx.middleware.filter)?,
                    ctx.middleware.cursor_limit,
                    200,
                    continue_token.as_deref(),
                )?;
                Ok(CommandResult::new(json!(page))
                    .with_cursor(continuation)
                    .with_next_actions(vec![
                    next_action(
                        "platform action describe <action>",
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
                     Run `gddy platform action list` for the list of valid action names.",
            )
            .with_system("actions")
            .with_tier(Tier::Read)
            .no_auth(true),
            |_cred, args: DescribeArgs| async move {
                let name = args.action;
                let schema = load_action_schema(&name).ok_or_else(|| {
                    cli_engine::CliCoreError::message(format!(
                        "action {name:?} not found; run `gddy platform action list` to see available actions"
                    ))
                })?;
                let result = protect_payload(schema, &format!("actions-describe-{name}"));
                let mut payload = result.value;
                if let Value::Object(ref mut map) = payload {
                    let mut gddy = serde_json::Map::new();
                    gddy.insert("truncated".to_string(), json!(result.metadata.is_some()));
                    if let Some(metadata) = result.metadata {
                        gddy.insert("total_bytes".to_string(), json!(metadata.total_bytes));
                        gddy.insert("shown_bytes".to_string(), json!(metadata.shown_bytes));

                        // Truncation should always produce a full_output file, but the write
                        // is best-effort and can fail (e.g. disk full, permission denied), so
                        // this stays an `if let` rather than an unconditional insert.
                        if let Some(full_output) = metadata.full_output {
                            gddy.insert("full_output".to_string(), json!(full_output));
                        }
                    }
                    // Namespaced under a reserved key so truncation metadata can never
                    // collide with a real property name in an action's JSON schema.
                    map.insert("_gddy".to_string(), Value::Object(gddy));
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
    async fn list_reports_cursor_metadata_via_cli_engine() {
        let cli = test_cli();
        let output = cli
            .run(["gddy", "platform", "actions", "list", "--output", "json"])
            .await;
        assert_eq!(output.exit_code, 0, "{}", output.rendered);
        let rendered: serde_json::Value =
            serde_json::from_str(&output.rendered).expect("stdout should contain json");
        let cursor = &rendered["cursor"];
        assert_eq!(cursor["total"], cursor["count"]);
        assert_eq!(cursor["has_more"], serde_json::json!(false));
    }

    /// `--filter` has to apply to the whole collection before it is sliced:
    /// the offset jump selects among the *matching* rows, `cursor.total` counts
    /// only those, and the replayed next-page action keeps the filter.
    #[tokio::test]
    async fn list_filter_applies_before_an_offset_jump_and_continuation() {
        let matching: Vec<&str> = super::ACTIONS
            .iter()
            .map(|(name, _)| *name)
            .filter(|name| name.starts_with("commerce."))
            .collect();
        assert!(
            matching.len() >= 3,
            "fixture needs several commerce.* actions"
        );
        assert!(
            matching.len() < super::ACTIONS.len(),
            "filter must exclude something"
        );

        let cli = test_cli();
        let first = cli
            .run([
                "gddy",
                "platform",
                "actions",
                "list",
                "--filter",
                "starts_with(name, 'commerce.')",
                "--offset",
                "1",
                "--limit",
                "1",
                "--output",
                "json",
            ])
            .await;
        assert_eq!(first.exit_code, 0, "{}", first.rendered);
        let first: serde_json::Value =
            serde_json::from_str(&first.rendered).expect("stdout should contain json");
        assert_eq!(first["data"].as_array().expect("array").len(), 1);
        assert_eq!(first["data"][0]["name"], matching[1]);
        assert_eq!(first["cursor"]["total"], matching.len());
        assert_eq!(first["cursor"]["remaining"], matching.len() - 2);

        let command = first["next_actions"]
            .as_array()
            .and_then(|actions| {
                actions
                    .iter()
                    .filter_map(|a| a["command"].as_str())
                    .find(|c| c.contains("--continue"))
            })
            .expect("a next-page action");
        assert!(
            command.contains("--filter"),
            "replay keeps the filter: {command}"
        );
        // The replay keeps the filter expression as one quoted token containing spaces,
        // so rebuild argv around it rather than splitting on whitespace.
        let (before, after) = command.split_once("--filter").expect("has --filter");
        let expr_start = after
            .find('"')
            .or_else(|| after.find('\''))
            .expect("quoted expr");
        let quote = after.as_bytes()[expr_start] as char;
        let expr_end = after[expr_start + 1..].find(quote).expect("closing quote") + expr_start + 1;
        let expr = &after[expr_start + 1..expr_end];
        let rest = &after[expr_end + 1..];
        let argv: Vec<&str> = before
            .split_whitespace()
            .chain(["--filter", expr])
            .chain(rest.split_whitespace().map(|part| part.trim_matches('"')))
            .chain(["--output", "json"])
            .collect();
        let second = cli.run(argv).await;
        assert_eq!(second.exit_code, 0, "{command}: {}", second.rendered);
        let second: serde_json::Value =
            serde_json::from_str(&second.rendered).expect("stdout should contain json");
        assert_eq!(second["data"][0]["name"], matching[2]);
    }

    /// The advertised next-page action replays `--offset` next to the new
    /// `--continue`; run exactly as printed it must advance, not fail to parse.
    #[tokio::test]
    async fn list_next_action_after_an_offset_jump_advances() {
        let cli = test_cli();
        let first = cli
            .run([
                "gddy", "platform", "actions", "list", "--limit", "3", "--offset", "3", "--output",
                "json",
            ])
            .await;
        assert_eq!(first.exit_code, 0, "{}", first.rendered);
        let first: serde_json::Value =
            serde_json::from_str(&first.rendered).expect("stdout should contain json");
        let command = first["next_actions"]
            .as_array()
            .and_then(|actions| {
                actions
                    .iter()
                    .filter_map(|a| a["command"].as_str())
                    .find(|c| c.contains("--continue"))
            })
            .expect("a next-page action");

        let argv: Vec<&str> = command
            .split_whitespace()
            .map(|part| part.trim_matches('"'))
            .chain(["--output", "json"])
            .collect();
        let second = cli.run(argv).await;
        assert_eq!(second.exit_code, 0, "{command}: {}", second.rendered);
        let second: serde_json::Value =
            serde_json::from_str(&second.rendered).expect("stdout should contain json");

        let names = |page: &serde_json::Value| -> Vec<String> {
            page["data"]
                .as_array()
                .expect("array")
                .iter()
                .map(|item| item["name"].as_str().unwrap_or_default().to_owned())
                .collect()
        };
        let expected: Vec<String> = super::ACTIONS[6..]
            .iter()
            .map(|(n, _)| (*n).to_owned())
            .collect();
        assert_eq!(
            names(&first),
            super::ACTIONS[3..6]
                .iter()
                .map(|(n, _)| (*n).to_owned())
                .collect::<Vec<_>>()
        );
        assert_eq!(names(&second), expected);
    }

    #[tokio::test]
    async fn list_limit_and_continue_page_through_the_catalog() {
        let cli = test_cli();
        let first = cli
            .run([
                "gddy", "platform", "actions", "list", "--limit", "3", "--output", "json",
            ])
            .await;
        assert_eq!(first.exit_code, 0, "{}", first.rendered);
        let first_json: serde_json::Value =
            serde_json::from_str(&first.rendered).expect("stdout should contain json");
        assert_eq!(first_json["data"].as_array().expect("array").len(), 3);
        assert_eq!(first_json["cursor"]["has_more"], serde_json::json!(true));
        let token = first_json["cursor"]["continue_from"]
            .as_str()
            .expect("has_more implies a token");

        let second = cli
            .run([
                "gddy",
                "platform",
                "actions",
                "list",
                "--limit",
                "3",
                "--continue",
                token,
                "--output",
                "json",
            ])
            .await;
        assert_eq!(second.exit_code, 0, "{}", second.rendered);
        let second_json: serde_json::Value =
            serde_json::from_str(&second.rendered).expect("stdout should contain json");
        assert_eq!(second_json["data"].as_array().expect("array").len(), 3);

        // An explicit --offset should land on the same page --continue's
        // token did, without needing the token at all.
        let via_offset = cli
            .run([
                "gddy", "platform", "actions", "list", "--limit", "3", "--offset", "3", "--output",
                "json",
            ])
            .await;
        assert_eq!(via_offset.exit_code, 0, "{}", via_offset.rendered);
        let via_offset_json: serde_json::Value =
            serde_json::from_str(&via_offset.rendered).expect("stdout should contain json");
        assert_eq!(via_offset_json["data"], second_json["data"]);
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
        assert_eq!(
            rendered["data"]["_gddy"]["truncated"],
            serde_json::json!(false)
        );
    }
}
