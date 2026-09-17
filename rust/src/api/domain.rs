//! `api domain list`.

use cli_engine::{CommandResult, CommandSpec, NextActionParam, RuntimeCommandSpec, Tier};
use serde_json::{Value, json};

use crate::next_action::next_action;
use crate::output_schema::output_schema;

use super::catalog::catalog;

output_schema!(ApiDomain {
    "domain": "string";
    "title": "string";
    "description": "string", optional;
    "endpoints": "number";
    "baseUrl": "string";
});

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_with_context(
        CommandSpec::new("list", "List all API domains")
            .with_long(
                "Lists every API domain in the embedded catalog. Use `api \
                operation list --domain <domain>` to drill into a specific \
                domain or `api search <query>` to find endpoints across all \
                domains.",
            )
            .with_system("api")
            .with_tier(Tier::Read)
            .no_auth(true)
            .with_default_fields("domain,title,endpoints,baseUrl")
            .with_output_schema::<ApiDomain>(),
        |ctx| async move {
            let catalog = catalog();
            let domains: Vec<Value> = catalog
                .iter()
                .map(|d| {
                    let base_url = crate::environments::resolve_catalog_base_url(
                        &d.name,
                        &d.base_url,
                        &ctx.middleware.env,
                    );
                    json!({
                        "domain": d.name,
                        "title": d.title,
                        "description": d.description,
                        "endpoints": d.endpoints.len(),
                        "baseUrl": base_url,
                    })
                })
                .collect();
            Ok(CommandResult::new(json!(domains)).with_next_actions(vec![
                next_action(
                    "api operation list --domain <domain>",
                    "List endpoints in a specific domain",
                )
                .with_param("domain", NextActionParam::required()),
                next_action("api search <query>", "Search across all endpoints"),
            ]))
        },
    )
}
