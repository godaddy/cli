use cli_engine::{CommandResult, CommandSpec, RuntimeCommandSpec, Tier};
use serde_json::{Value, json};

use crate::hosting::common::{HostingLogEntry, client_err, make_client, next_page_token};
use crate::scopes::HOSTING_LOG_READ as LOG_READ;

#[derive(Debug, Clone, clap::Args)]
struct LogListArgs {
    /// Application ID.
    #[arg(long = "app-id", value_name = "APP_ID")]
    app_id: String,

    /// Environment to retrieve logs from (PREVIEW or PUBLISH). Defaults to PREVIEW.
    #[arg(long, value_name = "VARIANT", value_parser = ["PREVIEW", "PUBLISH"])]
    variant: Option<String>,

    /// Return only entries at or after this ISO 8601 timestamp (e.g. 2024-01-01T00:00:00Z).
    #[arg(long, value_name = "DATETIME")]
    since: Option<String>,

    /// Filter by log source stream (STDOUT, STDERR, or ALL).
    #[arg(long, value_name = "SOURCE", value_parser = ["STDOUT", "STDERR", "ALL"])]
    source: Option<String>,

    /// Filter by severity level (INFO, WARN, or ERROR).
    #[arg(long, value_name = "LEVEL", value_parser = ["INFO", "WARN", "ERROR"])]
    level: Option<String>,

    /// Maximum number of log entries to return (max 500). Omit to return all.
    #[arg(long, value_name = "N", value_parser = clap::value_parser!(u32).range(1..=500))]
    limit: Option<u32>,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<LogListArgs, _, _, _>(
        CommandSpec::from_args::<LogListArgs>("list", "List application logs")
            .with_long(
                "Retrieve log entries for a hosting application. Results are autopaginated. \
                 Use --variant to select the environment, --since for a time window, \
                 --source and --level to filter by stream and severity.",
            )
            .with_system("hosting")
            .with_tier(Tier::Read)
            .with_scopes(&[LOG_READ])
            .with_default_fields("timestamp,level,message")
            .with_output_schema::<HostingLogEntry>(),
        |ctx, args: LogListArgs| async move {
            let limit = args.limit;
            let client = make_client(&ctx, &[LOG_READ]).await?;

            let mut all_items: Vec<Value> = Vec::new();
            let mut page_token: Option<String> = None;

            loop {
                let page_limit =
                    limit.map(|cap| cap.saturating_sub(all_items.len() as u32).min(500));

                let response = client
                    .list_logs(
                        &args.app_id,
                        args.variant.as_deref(),
                        args.since.as_deref(),
                        args.source.as_deref(),
                        args.level.as_deref(),
                        page_token.as_deref(),
                        page_limit,
                    )
                    .await
                    .map_err(client_err)?;

                if let Some(items) = response.get("items").and_then(|v| v.as_array()) {
                    all_items.extend(items.iter().cloned());
                }

                if limit.is_some_and(|cap| all_items.len() >= cap as usize) {
                    all_items.truncate(limit.expect("checked") as usize);
                    break;
                }

                match next_page_token(&response) {
                    Some(token) => page_token = Some(token),
                    None => break,
                }
            }

            Ok(CommandResult::new(json!(all_items)))
        },
    )
}
