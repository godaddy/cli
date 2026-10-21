use cli_engine::{CommandResult, CommandSpec, CursorConfig, RuntimeCommandSpec, Tier};
use serde_json::json;

use crate::hosting::common::{
    HostingLogEntry, client_err, make_client, page_continuation, page_items, page_limit,
};
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
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<LogListArgs, _, _, _>(
        CommandSpec::from_args::<LogListArgs>("list", "List application logs")
            .with_long(
                "Retrieve log entries for a hosting application, one page at a time. Use \
                 --limit to set the page size (max 500) and --continue with the token from \
                 a page's cursor to fetch the next. Use --variant to select the environment, --since for a time window, \
                 --source and --level to filter by stream and severity.",
            )
            .with_system("hosting")
            .with_tier(Tier::Read)
            .with_scopes(&[LOG_READ])
            .with_default_fields("timestamp,level,message")
            .with_output_schema::<HostingLogEntry>()
            .with_cursor(CursorConfig::new(100, 500)),
        |ctx, args: LogListArgs| async move {
            let client = make_client(&ctx, &[LOG_READ]).await?;

            let response = client
                .list_logs(
                    &args.app_id,
                    args.variant.as_deref(),
                    args.since.as_deref(),
                    args.source.as_deref(),
                    args.level.as_deref(),
                    ctx.middleware.continue_token.as_deref(),
                    page_limit(ctx.middleware.cursor_limit),
                )
                .await
                .map_err(client_err)?;

            Ok(CommandResult::new(json!(page_items(&response))).with_cursor(page_continuation(&response)))
        },
    )
}

#[cfg(test)]
mod tests {
    use cli_engine::CursorConfig;

    use super::command;

    #[test]
    fn opts_into_cursor_pagination_with_a_default_and_a_max_limit() {
        assert_eq!(command().spec.cursor, Some(CursorConfig::new(100, 500)));
    }
}
