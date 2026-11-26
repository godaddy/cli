use cli_engine::{CommandResult, CommandSpec, NextActionParam, RuntimeCommandSpec, Tier};
use serde_json::{Value, json};

use crate::error::GddyError;
use crate::hosting::common::{HostingServerSummary, client_err, make_client, next_page_token};
use crate::next_action::next_action;
use crate::scopes::HOSTING_SERVER_READ as SERVER_READ;

#[derive(Debug, Clone, clap::Args)]
struct VpsListArgs {
    /// Maximum number of servers to return. Omit to return all.
    #[arg(long, value_name = "N", value_parser = clap::value_parser!(u32).range(1..))]
    limit: Option<u32>,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<VpsListArgs, _, _, _>(
        CommandSpec::from_args::<VpsListArgs>("list", "List your virtual private servers")
            .with_long(
                "List every virtual private server in your account, across all data centers, \
                 in one call. Each row shows the server's name, ID, status, operating system, \
                 control panel, creation date and plan expiry date. Results are \
                 autopaginated: all pages are fetched and combined. Use --limit to \
                 cap the total returned.",
            )
            .with_system("hosting")
            .with_tier(Tier::Read)
            .with_scopes(&[SERVER_READ])
            .with_default_fields("name,id,status,os,controlPanel,createdAt,expiresAt")
            .with_output_schema::<HostingServerSummary>(),
        |ctx, args: VpsListArgs| async move {
            let client = make_client(&ctx, &[SERVER_READ]).await?;
            let mut all_items: Vec<Value> = Vec::new();
            let mut page_token: Option<String> = None;

            loop {
                let page_limit = args.limit.map(|cap| {
                    let remaining = cap.saturating_sub(all_items.len() as u32);
                    remaining.min(100)
                });

                let response = client
                    .list_servers(page_token.as_deref(), page_limit)
                    .await
                    .map_err(client_err)?;
                all_items.extend(page_items(&response)?);

                if let Some(cap) = args.limit
                    && all_items.len() >= cap as usize
                {
                    all_items.truncate(cap as usize);
                    break;
                }

                match next_page_token(&response) {
                    Some(token) => page_token = Some(token),
                    None => break,
                }
            }

            Ok(CommandResult::new(json!(all_items)).with_next_actions(vec![
                next_action(
                    "hosting vps get --server-id <server-id>",
                    "Show one server in detail",
                )
                .with_param("server-id", NextActionParam::required()),
            ]))
        },
    )
}

/// The `items` of one page. A response without an `items` array is a broken
/// answer, not an empty account, so it fails instead of listing nothing.
fn page_items(response: &Value) -> cli_engine::Result<Vec<Value>> {
    response
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .ok_or_else(|| {
            GddyError::unexpected("the servers response has no `items` list")
                .with_system("hosting")
                .into_cli_error()
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_with_servers_returns_them() {
        let items = page_items(&json!({ "items": [{ "id": "VPS-phx3-a" }], "links": [] }));
        assert_eq!(items.map(|i| i.len()).ok(), Some(1));
    }

    #[test]
    fn an_empty_page_is_an_empty_list() {
        let items = page_items(&json!({ "items": [] }));
        assert_eq!(items.map(|i| i.len()).ok(), Some(0));
    }

    #[test]
    fn a_response_without_items_is_an_error_not_an_empty_list() {
        for broken in [json!([]), json!({}), json!({ "items": "x" }), Value::Null] {
            assert!(page_items(&broken).is_err(), "{broken}");
        }
    }
}
