//! `gddy domain list` — list the domains in the account (v1).

use cli_engine::{
    CliCoreError, CommandResult, CommandSpec, NextActionParam, PaginationConfig, Result,
    RuntimeCommandSpec, Tier,
};
use serde_json::json;

use domains_client::types;

use super::common::{api_error, make_client};
use crate::next_action::next_action;
use crate::scopes::DOMAINS_READ;

/// Validate `--status` values case-insensitively against the generated
/// `ListStatusesItem` enum (the v1 list API's `DomainStatus` set, e.g. `ACTIVE`).
fn parse_statuses(raw: &[String]) -> Result<Vec<types::ListStatusesItem>> {
    raw.iter()
        .map(|s| {
            types::ListStatusesItem::try_from(s.to_uppercase().as_str())
                .map_err(|_| CliCoreError::message(format!("invalid --status {s:?}")))
        })
        .collect()
}

/// Whether the request should be scoped to the `VISIBLE` status group —
/// GoDaddy's default view that hides cancelled/confiscated/other
/// non-visible domains. Skipped when the caller passed an explicit
/// `--status` filter or asked to see hidden domains via `--show-hidden`.
fn wants_visible_only(statuses: &[types::ListStatusesItem], show_hidden: bool) -> bool {
    statuses.is_empty() && !show_hidden
}

#[derive(Debug, Clone, clap::Args)]
struct ListArgs {
    /// Only domains with this status, e.g. ACTIVE (repeatable).
    #[arg(long, value_name = "STATUS")]
    status: Vec<String>,

    /// Include domains hidden by default, e.g. cancelled or confiscated.
    #[arg(long)]
    show_hidden: bool,
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<ListArgs, _, _, _>(
        CommandSpec::from_args::<ListArgs>("list", "List the domains in your account")
            .with_long(
                "List the domains registered to your account. Shows domain, status, \
                 expiry, and auto-renew by default; use --fields to pick columns. Hides \
                 domains that are cancelled or otherwise not visible unless --show-hidden \
                 is passed; use --status to filter to specific status values (repeatable).",
            )
            .with_system("domain")
            .with_tier(Tier::Read)
            .with_default_fields("domain,status,expires,renewAuto")
            .with_json_schema::<types::V1DomainSummary>()
            .with_scopes(&[DOMAINS_READ])
            .with_pagination(PaginationConfig {
                max_limit: 500,
                ..Default::default()
            }),
        |ctx, args: ListArgs| async move {
            let debug = !ctx.middleware.debug.is_empty();
            let statuses = parse_statuses(&args.status)?;
            let show_hidden = args.show_hidden;
            let client = make_client(&ctx).await?;
            let mut req = client.list();
            if !statuses.is_empty() {
                req = req.statuses(statuses);
            } else if wants_visible_only(&statuses, show_hidden) {
                req = req.status_groups(vec![types::ListStatusGroupsItem::Visible]);
            }
            let resp = match req.send().await {
                Ok(r) => r,
                Err(e) => return Err(api_error("listing domains", debug, e).await),
            };
            let domains: Vec<serde_json::Value> = resp
                .into_inner()
                .iter()
                .map(serde_json::to_value)
                .collect::<std::result::Result<_, _>>()
                .map_err(|e| {
                    CliCoreError::message(format!("failed to serialize domain list: {e}"))
                })?;
            Ok(CommandResult::new(json!(domains)).with_next_actions(vec![
                next_action("dns list <domain>", "View a domain's DNS records")
                    .with_param("domain", NextActionParam::required()),
            ]))
        },
    )
}

#[cfg(test)]
mod tests {
    use super::{command, parse_statuses, wants_visible_only};
    use cli_engine::PaginationConfig;
    use domains_client::types;

    /// Regression pin: `domain list` opts into pagination with no forced
    /// `default_limit` (an unflagged invocation must keep returning every
    /// domain, exactly like before this feature existed) but a `max_limit`
    /// so `--limit` can't be pointed at an absurd value.
    #[test]
    fn opts_into_pagination_with_no_default_and_a_max_limit() {
        assert_eq!(
            command().spec.pagination,
            Some(PaginationConfig {
                max_limit: 500,
                ..Default::default()
            })
        );
    }

    #[test]
    fn parse_statuses_is_case_insensitive_and_validates() {
        use types::ListStatusesItem;
        let parsed = parse_statuses(&["active".to_string(), "CANCELLED".to_string()])
            .expect("valid statuses");
        assert_eq!(
            parsed,
            vec![ListStatusesItem::Active, ListStatusesItem::Cancelled]
        );
        assert!(parse_statuses(&[]).expect("empty ok").is_empty());
        let err = parse_statuses(&["bogus".to_string()]).expect_err("should reject");
        assert!(err.to_string().contains("invalid --status"), "{err}");
    }

    #[test]
    fn wants_visible_only_defaults_true_but_yields_to_status_or_show_hidden() {
        assert!(wants_visible_only(&[], false));
        assert!(!wants_visible_only(&[], true));
        assert!(!wants_visible_only(
            &[types::ListStatusesItem::Cancelled],
            false
        ));
    }
}
