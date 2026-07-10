//! `gddy domain agreements` — the legal agreements a TLD requires (v1).

use cli_engine::{
    CommandResult, CommandSpec, NextActionParam, RuntimeCommandSpec, TableColumn, Tier,
};
use serde_json::json;

use domains_client::types;

use super::common::{api_error, make_client, string_list};
use crate::next_action::next_action;
use crate::scopes::DOMAINS_READ;

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_with_context(
        CommandSpec::new(
            "agreements",
            "Show the legal agreements required to register a TLD",
        )
        .with_long(
            "List the legal agreements you must consent to before registering under \
                 a TLD. `domain quote` also returns the agreements specific to a domain; \
                 this is the TLD-level view.",
        )
        .with_system("domain")
        .with_tier(Tier::Read)
        .with_default_fields("agreementKey,title,url")
        .with_view(vec![
            TableColumn::new("agreementKey", "Agreement Key"),
            TableColumn::new("title", "Title"),
            TableColumn::new("url", "URL").no_truncate(true),
        ])
        .with_json_schema::<types::V1LegalAgreement>()
        .with_scopes(&[DOMAINS_READ])
        .with_arg(
            clap::Arg::new("tld")
                .long("tld")
                .value_name("TLD")
                .required(true)
                .action(clap::ArgAction::Append)
                .help("TLD whose agreements to retrieve, e.g. com (repeatable)"),
        )
        .with_arg(
            clap::Arg::new("privacy")
                .long("privacy")
                .action(clap::ArgAction::SetTrue)
                .help("Retrieve the agreements that apply when privacy is requested"),
        ),
        |ctx| async move {
            let tlds = string_list(&ctx, "tld");
            let privacy = ctx
                .args
                .get("privacy")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let debug = !ctx.middleware.debug.is_empty();
            let client = make_client(&ctx).await?;
            let resp = match client.agreements().tlds(tlds).privacy(privacy).send().await {
                Ok(r) => r,
                Err(e) => return Err(api_error("retrieving legal agreements", debug, e).await),
            };
            let agreements: Vec<serde_json::Value> = resp
                .into_inner()
                .into_iter()
                .map(|a| {
                    json!({
                        "agreementKey": a.agreement_key,
                        "title": a.title,
                        "url": a.url,
                        "content": a.content,
                    })
                })
                .collect();
            Ok(
                CommandResult::new(json!(agreements)).with_next_actions(vec![
                    next_action(
                        "domain quote <domain>",
                        "Price a registration and see the agreements for a specific domain",
                    )
                    .with_param("domain", NextActionParam::required()),
                    next_action(
                        "domain purchase --quote-token <quote-token> --agree --confirm",
                        "Register once you have a quote",
                    )
                    .with_param("quote-token", NextActionParam::required()),
                ]),
            )
        },
    )
}
