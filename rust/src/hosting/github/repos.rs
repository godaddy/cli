use cli_engine::{CommandResult, CommandSpec, NextActionParam, RuntimeCommandSpec, Tier};
use serde_json::{Value, json};

use crate::hosting::common::{client_err, make_client, next_page_token};
use crate::next_action::next_action;
use crate::scopes::HOSTING_GITHUB_READ as GH_READ;

#[derive(Debug, Clone, clap::Args)]
struct ReposArgs {
    /// Maximum number of repositories to return. Omit to return all.
    #[arg(long, value_name = "N", value_parser = clap::value_parser!(u32).range(1..))]
    limit: Option<u32>,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<ReposArgs, _, _, _>(
        CommandSpec::from_args::<ReposArgs>("repos", "List connected GitHub repositories")
            .with_long(
                "List repositories accessible via the connected GitHub account. \
                 Results are autopaginated.",
            )
            .with_system("hosting")
            .with_tier(Tier::Read)
            .with_scopes(&[GH_READ])
            .with_default_fields("fullName"),
        |ctx, args: ReposArgs| async move {
            let limit = args.limit;
            let client = make_client(&ctx, &[GH_READ]).await?;

            let mut all_items: Vec<Value> = Vec::new();
            let mut page_token: Option<String> = None;

            loop {
                let page_limit =
                    limit.map(|cap| cap.saturating_sub(all_items.len() as u32).min(100));

                let response = client
                    .list_github_repos(page_token.as_deref(), page_limit)
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

            Ok(
                CommandResult::new(json!({ "items": all_items })).with_next_actions(vec![
                    next_action(
                        "hosting github branches --owner <owner> --repo <repo>",
                        "List branches for a repository",
                    )
                    .with_param("owner", NextActionParam::required())
                    .with_param("repo", NextActionParam::required()),
                ]),
            )
        },
    )
}
