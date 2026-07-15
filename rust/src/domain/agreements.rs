//! `gddy domain agreements` — the legal agreements a TLD requires (v1).

use cli_engine::{
    CommandResult, CommandSpec, NextActionParam, RuntimeCommandSpec, TableColumn, Tier,
};
use serde_json::json;

use domains_client::types;

use super::common::{api_error, comma_joined, make_client, string_list};
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
            let resp = match client
                .agreements()
                .tlds(comma_joined(tlds))
                .privacy(privacy)
                .send()
                .await
            {
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

#[cfg(test)]
mod tests {
    use super::super::common::comma_joined;

    #[tokio::test]
    async fn tlds_are_sent_as_a_single_comma_joined_query_param() {
        // Regression for DEVEX-882: the v1 `tlds` query param is OpenAPI
        // `style: form, explode: false` — one comma-joined value. progenitor's
        // generated `tlds()` setter always seq-serializes a `Vec` as repeated
        // `tlds=` pairs, so callers must join multiple `--tld` occurrences into
        // a single element before calling it, or the API rejects the request.
        let server = httpmock::MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::GET)
                    .path("/v1/domains/agreements")
                    .query_param("tlds", "com,net,io");
                then.status(200).json_body(serde_json::json!([]));
            })
            .await;
        let client =
            domains_client::client_with_auth(&server.base_url(), "Bearer tok", "test", "req-1")
                .expect("build client");

        let tlds = comma_joined(vec!["com".to_string(), "net".to_string(), "io".to_string()]);
        client
            .agreements()
            .tlds(tlds)
            .privacy(false)
            .send()
            .await
            .expect("request succeeds");

        mock.assert_async().await;
    }
}
