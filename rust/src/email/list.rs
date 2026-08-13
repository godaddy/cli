use cli_engine::{CommandResult, CommandSpec, NextActionParam, RuntimeCommandSpec, Tier};

use crate::email::{client_err, make_client};
use crate::next_action::next_action;
use crate::scopes::EMAIL_READ;

#[derive(Debug, Clone, clap::Args)]
struct ListArgs {
    #[arg(long, value_name = "STATUS")]
    status: Option<String>,
    #[arg(long, value_name = "PAGE")]
    page: Option<u32>,
    #[arg(long = "page-size", value_name = "PAGE_SIZE")]
    page_size: Option<u32>,
    #[arg(long, value_name = "FIELDS")]
    fields: Option<String>,
}

/// Query params are forwarded 1:1 to the panel API rather than translated
/// through `PaginationConfig`'s generic `--limit`/`--offset` model — the
/// server paginates natively (`page`/`pageSize`) and returns HATEOAS `links`
/// that offset math would hide. See `docs/proposals/email-management-cli.md`.
pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<ListArgs, _, _, _>(
        CommandSpec::from_args::<ListArgs>("list", "List your Email mailboxes")
            .with_system("email")
            .with_tier(Tier::Read)
            .with_scopes(&[EMAIL_READ]),
        |ctx, args: ListArgs| async move {
            let client = make_client(&ctx, &[EMAIL_READ]).await?;
            let mut query: Vec<(&str, String)> = Vec::new();
            if let Some(status) = args.status {
                query.push(("status", status));
            }
            if let Some(page) = args.page {
                query.push(("page", page.to_string()));
            }
            if let Some(page_size) = args.page_size {
                query.push(("pageSize", page_size.to_string()));
            }
            if let Some(fields) = args.fields {
                query.push(("fields", fields));
            }
            let data = client.list_mailboxes(&query).await.map_err(client_err)?;
            Ok(CommandResult::new(data).with_next_actions(vec![
                next_action("email get <mailbox-id>", "Get a mailbox by ID")
                    .with_param("mailbox-id", NextActionParam::required()),
            ]))
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_a_read_tier_command_scoped_to_email_read() {
        let spec = command().spec;
        assert_eq!(spec.tier, Some(Tier::Read));
        assert_eq!(spec.metadata().scopes, vec![EMAIL_READ.to_string()]);
    }
}
